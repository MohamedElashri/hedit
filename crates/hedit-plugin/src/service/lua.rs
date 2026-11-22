//! The same restricted Lua engine, with typed provider context and results.
use super::*;
use mlua::{Lua, Table};
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
use crate::structured::{from_lua, to_lua, Budget};
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
