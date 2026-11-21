//! Resource changes share the command-policy lock and invalidate every captured
//! broker epoch. Neither workspace files nor request packets can create grants.
use super::*;
use hedit_broker::{
    state::{Approval, State},
    Handle,
};
fn token(grant: &Grant) -> Result<Handle, String> {
    serde_json::from_value(grant.approval_id().into())
        .map_err(|_| "Invalid source approval token.".into())
}
pub(in crate::app::extensions) fn resource_approvals(
    config: &Path,
) -> Result<BTreeMap<String, Approval>, String> {
    if !State::exists(config) {
        return Ok(BTreeMap::new());
    }
    State::open(config, [])
        .and_then(|state| state.approvals())
        .map_err(|error| error.to_string())
}
pub(in crate::app::extensions) fn broker_epoch(
    config: &Path,
    lock: &File,
) -> Result<Vec<u8>, String> {
    permissions::broker_epoch(config, lock)
}
pub(in crate::app::extensions) fn broker_authorization(
    config: &Path,
    plugin: &LuaPlugin,
) -> Option<File> {
    if plugin.broker.is_none() || plugin.is_one_time() || plugin.storage {
        return None;
    }
    authorization_with_lock(config, plugin, permissions::disclosure_lock(config).ok()?)
}
pub(in crate::app::extensions) fn approve_resources(
    config: &Path,
    path: &Path,
    id: &str,
    digest: &str,
    approval: Approval,
) -> Result<Catalog, String> {
    let lock = permissions::mutation_lock(config)?;
    let (installed, manifest) = current_lua_command(config, path, id, &approval.command)?;
    if installed.digest != digest || disabled(config)?.contains(id) {
        return Err("The extension changed. Review its resource requests again.".into());
    }
    let definition = manifest
        .broker(&approval.command)
        .ok_or("This command has no broker declaration.")?;
    let grants = Grants::read(config)?;
    let grant = grants
        .commands
        .get(&approval.command)
        .filter(|grant| grant.matches(&installed))
        .ok_or("Approve this exact command before reviewing resources.")?;
    if token(grant)? != approval.token {
        return Err("The source approval changed. Review resources again.".into());
    }
    approval
        .validate_definition(definition)
        .map_err(|error| error.to_string())?;
    advance_one_time_epoch(&lock)?;
    let state = State::open(config, []).map_err(|error| error.to_string())?;
    // Resolve the selected physical root again before persisting its approval.
    if let Some(scope) = &approval.scope {
        let current = hedit_broker::state::SelectedScope::select(
            scope.path(),
            scope.rights().to_vec(),
            &state,
        )
        .map_err(|error| error.to_string())?;
        if &current != scope {
            return Err("The selected directory changed. Select it again.".into());
        }
    }
    state
        .approve(approval, definition)
        .map_err(|error| error.to_string())?;
    Ok(read(config))
}
pub(in crate::app::extensions) fn revoke_resources(
    config: &Path,
    command: &str,
) -> Result<Catalog, String> {
    let lock = permissions::mutation_lock(config)?;
    advance_one_time_epoch(&lock)?;
    if State::exists(config) {
        State::open(config, [])
            .and_then(|state| state.revoke(command))
            .map_err(|error| error.to_string())?;
    }
    Ok(read(config))
}
pub(super) fn grant_token(grant: &Grant) -> Option<Handle> {
    token(grant).ok()
}
pub(in crate::app::extensions) fn reset_resource_approvals(
    config: &Path,
) -> Result<Catalog, String> {
    let _lock = permissions::mutation_lock(config)?;
    if resource_approvals(config).is_ok() {
        return Err("Resource approvals changed. Reload before resetting them.".into());
    }
    State::open(config, [])
        .and_then(|state| state.reset())
        .map_err(|error| error.to_string())?;
    Ok(read(config))
}
