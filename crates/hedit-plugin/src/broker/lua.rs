//! Lua returns an intent then exits. No VM waits for broker I/O.
use super::*;
use crate::structured::{from_lua, to_lua, Budget};
use mlua::{Lua, Table};
use std::{cell::RefCell, rc::Rc};
fn failure(_: mlua::Error) -> Error {
    Error::Execution
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    intent: Intent,
    #[serde(default)]
    cache: Option<sdk::Storage>,
}
pub(crate) fn evaluate(
    lua: &Lua,
    environment: Table,
    input: Input,
    location: Rc<RefCell<Option<sdk::Location>>>,
) -> Result<Output, Error> {
    input.validate()?;
    let context = serde_json::json!({"scope":input.scope,"request":input.request,"step":input.step,"response":input.response});
    let cache = serde_json::to_value(&input.cache).map_err(|_| Error::Request)?;
    let provider = serde_json::to_value(&input.provider).map_err(|_| Error::Request)?;
    let (value, _, _) = sdk::lua::invoke(
        lua,
        environment,
        input.package.clone(),
        location,
        |lua, context_table, api| {
            super::json::install(lua, api).map_err(failure)?;
            context_table
                .raw_set("broker", to_lua(lua, &context).map_err(failure)?)
                .map_err(failure)?;
            context_table
                .raw_set("cache", to_lua(lua, &cache).map_err(failure)?)
                .map_err(failure)?;
            context_table
                .raw_set("request", to_lua(lua, &provider).map_err(failure)?)
                .map_err(failure)?;
            Ok(())
        },
    )?;
    let proposal: Proposal =
        serde_json::from_value(from_lua(value, "", 0, &mut Budget { bytes: 0, nodes: 0 })?)
            .map_err(|_| Error::ReturnType)?;
    let output = Output {
        request: input.request,
        step: input.step,
        intent: proposal.intent,
        cache: proposal.cache.unwrap_or(input.cache.clone()),
    };
    output.validate(&input)?;
    Ok(output)
}
