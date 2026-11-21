//! Pure bounded JSON helpers for endpoint integrations. No metatable execution,
//! lossy integer conversion, duplicate keys, ambient I/O, or retained VM state.
use mlua::{LightUserData, Lua, Table, Value};
use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Number, Value as Json};
const MAX: usize = 64 * 1024;
const ITEMS: usize = 512;
const ARRAY: &str = "__hedit_json_array";
fn null() -> LightUserData {
    LightUserData(std::ptr::without_provenance_mut(1))
}
fn failed() -> mlua::Error {
    mlua::Error::RuntimeError("JSON data is invalid or exceeds its limit.".into())
}
#[derive(Default)]
struct Budget {
    nodes: usize,
    bytes: usize,
}
impl Budget {
    fn node(&mut self, depth: usize) -> Result<(), ()> {
        self.nodes += 1;
        if depth > 16 || self.nodes > 4096 {
            Err(())
        } else {
            Ok(())
        }
    }
    fn bytes(&mut self, bytes: usize) -> Result<(), ()> {
        self.bytes = self.bytes.checked_add(bytes).ok_or(())?;
        if self.bytes > MAX {
            Err(())
        } else {
            Ok(())
        }
    }
}
struct Seed<'a> {
    budget: &'a mut Budget,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Json;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Json, D::Error> {
        self.budget
            .node(self.depth)
            .map_err(|_| D::Error::custom("JSON limit"))?;
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Json;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("bounded JSON data")
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Json, E> {
        Ok(v.into())
    }
    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Json, E> {
        Ok(v.into())
    }
    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Json, E> {
        if v > i64::MAX as u64 {
            return Err(E::custom("JSON integer limit"));
        }
        Ok(v.into())
    }
    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Json, E> {
        if v.abs() >= 9_223_372_036_854_775_808.0 {
            return Err(E::custom("JSON number limit"));
        }
        Number::from_f64(v)
            .map(Json::Number)
            .ok_or_else(|| E::custom("JSON number limit"))
    }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Json, E> {
        self.budget
            .bytes(v.len())
            .map_err(|_| E::custom("JSON limit"))?;
        Ok(v.into())
    }
    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Json, E> {
        self.budget
            .bytes(v.len())
            .map_err(|_| E::custom("JSON limit"))?;
        Ok(Json::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(Seed {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            if values.len() >= ITEMS {
                return Err(A::Error::custom("JSON array limit"));
            }
            values.push(value);
        }
        Ok(Json::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            self.budget
                .bytes(key.len())
                .map_err(|_| A::Error::custom("JSON limit"))?;
            if values.len() >= ITEMS || values.contains_key(&key) {
                return Err(A::Error::custom("JSON object limit or duplicate key"));
            }
            let value = map.next_value_seed(Seed {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Json::Object(values))
    }
}
pub(crate) fn decode(bytes: &[u8]) -> Result<Json, ()> {
    if bytes.len() > MAX {
        return Err(());
    }
    let mut budget = Budget::default();
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = Seed {
        budget: &mut budget,
        depth: 0,
    }
    .deserialize(&mut decoder)
    .map_err(|_| ())?;
    decoder.end().map_err(|_| ())?;
    Ok(value)
}
fn mark_array(lua: &Lua, table: &Table) -> mlua::Result<()> {
    if let Some(metatable) = table.metatable() {
        return if metatable.raw_get::<bool>(ARRAY).unwrap_or(false) {
            Ok(())
        } else {
            Err(failed())
        };
    }
    let metatable = lua.create_table()?;
    metatable.raw_set(ARRAY, true)?;
    metatable.raw_set("__metatable", "JSON array")?;
    table.set_metatable(Some(metatable))
}
fn to_lua(lua: &Lua, value: &Json) -> mlua::Result<Value> {
    Ok(match value {
        Json::Null => Value::LightUserData(null()),
        Json::Bool(value) => Value::Boolean(*value),
        Json::String(value) => Value::String(lua.create_string(value)?),
        Json::Number(value) => {
            if let Some(value) = value.as_i64() {
                Value::Integer(value)
            } else if value.is_f64() {
                Value::Number(value.as_f64().ok_or_else(failed)?)
            } else {
                return Err(failed());
            }
        }
        Json::Array(values) => {
            let table = lua.create_table()?;
            for (index, value) in values.iter().enumerate() {
                table.raw_set(index + 1, to_lua(lua, value)?)?;
            }
            mark_array(lua, &table)?;
            Value::Table(table)
        }
        Json::Object(values) => {
            let table = lua.create_table()?;
            for (key, value) in values {
                table.raw_set(key.as_str(), to_lua(lua, value)?)?;
            }
            Value::Table(table)
        }
    })
}
fn from_lua(value: Value, depth: usize, budget: &mut Budget) -> mlua::Result<Json> {
    budget.node(depth).map_err(|_| failed())?;
    Ok(match value {
        Value::LightUserData(value) if value == null() => Json::Null,
        Value::Boolean(value) => value.into(),
        Value::Integer(value) => value.into(),
        Value::Number(value) => {
            if value.abs() >= 9_223_372_036_854_775_808.0 {
                return Err(failed());
            }
            Json::Number(Number::from_f64(value).ok_or_else(failed)?)
        }
        Value::String(value) => {
            budget.bytes(value.as_bytes().len()).map_err(|_| failed())?;
            Json::String(value.to_str().map_err(|_| failed())?.to_owned())
        }
        Value::Table(table) => {
            let len = table.raw_len();
            let array = len > 0
                || table
                    .metatable()
                    .is_some_and(|table| table.raw_get::<bool>(ARRAY).unwrap_or(false));
            if array {
                if len > ITEMS {
                    return Err(failed());
                }
                let mut count = 0;
                for pair in table.clone().pairs::<Value, Value>() {
                    let (key, _) = pair.map_err(|_| failed())?;
                    let Value::Integer(key) = key else {
                        return Err(failed());
                    };
                    if key < 1 || key as usize > len {
                        return Err(failed());
                    }
                    count += 1;
                    if count > ITEMS {
                        return Err(failed());
                    }
                }
                if count != len {
                    return Err(failed());
                }
                let mut values = Vec::with_capacity(len);
                for index in 1..=len {
                    values.push(from_lua(table.raw_get(index)?, depth + 1, budget)?);
                }
                Json::Array(values)
            } else {
                let mut values = serde_json::Map::new();
                for pair in table.pairs::<Value, Value>() {
                    if values.len() >= ITEMS {
                        return Err(failed());
                    }
                    let (key, value) = pair.map_err(|_| failed())?;
                    let Value::String(key) = key else {
                        return Err(failed());
                    };
                    budget.bytes(key.as_bytes().len()).map_err(|_| failed())?;
                    let key = key.to_str().map_err(|_| failed())?.to_owned();
                    values.insert(key, from_lua(value, depth + 1, budget)?);
                }
                Json::Object(values)
            }
        }
        _ => return Err(failed()),
    })
}
pub(super) fn install(lua: &Lua, api: &Table) -> mlua::Result<()> {
    api.raw_set("json_null", null())?;
    api.raw_set(
        "json_array",
        lua.create_function(|lua, table: Table| {
            mark_array(lua, &table)?;
            Ok(table)
        })?,
    )?;
    api.raw_set(
        "json_encode",
        lua.create_function(|lua, value: Value| {
            let json = from_lua(value, 0, &mut Budget::default())?;
            let bytes = serde_json::to_vec(&json).map_err(|_| failed())?;
            if bytes.len() > MAX {
                return Err(failed());
            }
            lua.create_string(bytes)
        })?,
    )?;
    api.raw_set(
        "json_decode",
        lua.create_function(|lua, value: mlua::LuaString| {
            let value = decode(&value.as_bytes()).map_err(|_| failed())?;
            to_lua(lua, &value)
        })?,
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoding_is_strict_and_bounded_before_lua_allocation() {
        for data in [
            br#"{"a":1,"a":2}"#.as_slice(),
            b"[",
            b"[1,]",
            b"18446744073709551615",
            b"1e400",
            b"\"\xff\"",
        ] {
            assert!(decode(data).is_err());
        }
        assert!(decode(&vec![b' '; MAX + 1]).is_err());
        assert!(decode(format!("[{}]", vec!["0"; ITEMS + 1].join(",")).as_bytes()).is_err());
        assert!(decode(format!("{}0{}", "[".repeat(18), "]".repeat(18)).as_bytes()).is_err());
    }
    #[test]
    fn json_round_trips_null_arrays_unicode_and_finite_numbers_and_rejects_cycles() {
        crate::runtime::engine::with_lua(|lua,environment|{
            let api=lua.create_table().map_err(|_|crate::Error::Execution)?;install(lua,&api).map_err(|_|crate::Error::Execution)?;environment.raw_set("hedit",api).map_err(|_|crate::Error::Execution)?;
            let result:String=lua.load(r#"local object=hedit.json_decode('{"null":null,"array":[],"name":"東京","confidence":0.75}'); assert(object.null==hedit.json_null); assert(hedit.json_encode(object.array)=='[]'); return hedit.json_encode(object)"#).set_environment(environment.clone()).eval().map_err(|_|crate::Error::Execution)?;
            assert_eq!(decode(result.as_bytes()).unwrap(),serde_json::json!({"null":null,"array":[],"name":"東京","confidence":0.75}));
            for source in ["local t={}; t.self=t; return hedit.json_encode(t)","return hedit.json_encode({[2]='hole'})","return hedit.json_encode(0/0)","return hedit.json_encode(string.char(255))"] {assert!(lua.load(source).set_environment(environment.clone()).eval::<Value>().is_err());}
            Ok(())
        }).unwrap();
    }
}
