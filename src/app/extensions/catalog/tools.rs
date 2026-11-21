//! Native image selection stores a private copy, never an execution grant.
use super::*;
use hedit_broker::{
    state::State,
    tool::{Binding, Info, Source},
    Handle,
};
use std::sync::atomic::AtomicBool;
fn binding(
    config: &Path,
    path: &Path,
    extension: &str,
    command: &str,
    digest: &str,
    expected: Handle,
    profile: &str,
) -> Result<Binding, String> {
    let (installed, manifest) = current_lua_command(config, path, extension, command)?;
    if installed.digest != digest || disabled(config)?.contains(extension) {
        return Err("The extension changed. Review this tool request again.".into());
    }
    let grants = Grants::read(config)?;
    let grant = grants
        .commands
        .get(command)
        .filter(|grant| grant.matches(&installed))
        .ok_or("Approve this exact command before selecting tools.")?;
    if resources::grant_token(grant) != Some(expected) {
        return Err("The source approval changed. Review resources again.".into());
    }
    let recipe = manifest
        .broker(command)
        .and_then(|definition| definition.tools.get(profile))
        .ok_or("This native tool profile is no longer declared.")?;
    Binding::new(command, expected, profile, recipe).map_err(|error| error.to_string())
}
pub(in crate::app::extensions) struct Selection<'a> {
    pub config: &'a Path,
    pub path: &'a Path,
    pub extension: &'a str,
    pub command: &'a str,
    pub digest: &'a str,
    pub token: Handle,
    pub profile: &'a str,
}
impl Selection<'_> {
    fn binding(&self) -> Result<Binding, String> {
        binding(
            self.config,
            self.path,
            self.extension,
            self.command,
            self.digest,
            self.token,
            self.profile,
        )
    }
}
pub(in crate::app::extensions) fn import_tool(
    selection: Selection<'_>,
    selected: &Path,
    cancel: &AtomicBool,
) -> Result<(Handle, Info), String> {
    let _guard = permissions::disclosure_lock(selection.config)?;
    let binding = selection.binding()?;
    let state = State::open(selection.config, []).map_err(|error| error.to_string())?;
    let store = state.tool_store().map_err(|error| error.to_string())?;
    let source = Source::select(selected).map_err(|error| error.to_string())?;
    let id = store
        .import(source, binding.clone(), cancel)
        .map_err(|error| error.to_string())?;
    let info = store
        .info(id, &binding)
        .map_err(|error| error.to_string())?;
    Ok((id, info))
}
pub(in crate::app::extensions) fn tool_info(
    selection: Selection<'_>,
    image: Handle,
) -> Result<Info, String> {
    let _guard = permissions::disclosure_lock(selection.config)?;
    let binding = selection.binding()?;
    State::open(selection.config, [])
        .and_then(|state| state.tool_store())
        .and_then(|store| store.info(image, &binding))
        .map_err(|error| error.to_string())
}
pub(in crate::app::extensions) fn clear_unused_tools(config: &Path) -> Result<Catalog, String> {
    let guard = permissions::mutation_lock(config)?;
    advance_one_time_epoch(&guard)?;
    let state = State::open(config, []).map_err(|error| error.to_string())?;
    let referenced = state
        .approvals()
        .map_err(|error| error.to_string())?
        .values()
        .flat_map(|approval| approval.tools.values().copied())
        .collect();
    state
        .tool_store()
        .and_then(|store| store.remove_unused(&referenced))
        .map_err(|error| error.to_string())?;
    Ok(read(config))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::app::extensions::manifest::{self, ExtensionAction};
    #[test]
    fn selecting_an_image_does_not_grant_it_and_reapproval_cannot_reuse_foreign_source_authority() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path();
        std::fs::create_dir(config.join("extensions")).unwrap();
        let manifest = serde_json::json!({
            "api_version":5,"sdk_version":3,"id":"native-test",
            "commands":[{"id":"run","title":"Native test","broker":{"capabilities":["tools.run"],"tools":{"formatter":{"protocol":"filter","workspace":false,"arguments":[{"literal":"a-z"},{"literal":"A-Z"}]}}},"action":{"kind":"lua","source":"run.lua"}}],
            "scripts":{"run.lua":"return function(ctx) return {intent={finish={}}} end"}
        }).to_string();
        let path = config.join("extensions/native-test.json");
        std::fs::write(&path, &manifest).unwrap();
        let digest = manifest::digest(&manifest);
        let approved = set_lua_approved(
            config,
            &path,
            "native-test",
            "native-test.run",
            &digest,
            true,
        )
        .unwrap();
        let ExtensionAction::Lua(plugin) = &approved.contributions[0].action else {
            panic!()
        };
        let token = resources::grant_token(&plugin.grant).unwrap();
        let select = || Selection {
            config,
            path: &path,
            extension: "native-test",
            command: "native-test.run",
            digest: &digest,
            token,
            profile: "formatter",
        };
        approve_resources(
            config,
            &path,
            "native-test",
            &digest,
            hedit_broker::state::Approval {
                command: "native-test.run".into(),
                token,
                scope: None,
                endpoints: vec![],
                tools: Default::default(),
            },
        )
        .unwrap();
        let (image, info) =
            import_tool(select(), Path::new("/usr/bin/tr"), &AtomicBool::new(false)).unwrap();
        assert_eq!(info.origin, "tr");
        assert_eq!(info.sha256.len(), 64);
        let state = State::open(config, []).unwrap();
        assert!(state
            .resolve("native-test.run", token, plugin.broker.as_ref().unwrap())
            .unwrap()
            .tools
            .is_empty());
        approve_resources(
            config,
            &path,
            "native-test",
            &digest,
            hedit_broker::state::Approval {
                command: "native-test.run".into(),
                token,
                scope: None,
                endpoints: vec![],
                tools: BTreeMap::from([("formatter".into(), image)]),
            },
        )
        .unwrap();
        assert_eq!(
            state
                .resolve("native-test.run", token, plugin.broker.as_ref().unwrap())
                .unwrap()
                .tools
                .len(),
            1
        );
        set_lua_approved(
            config,
            &path,
            "native-test",
            "native-test.run",
            &digest,
            false,
        )
        .unwrap();
        assert!(tool_info(select(), image).is_err());
        let next = set_lua_approved(
            config,
            &path,
            "native-test",
            "native-test.run",
            &digest,
            true,
        )
        .unwrap();
        let ExtensionAction::Lua(next) = &next.contributions[0].action else {
            panic!()
        };
        let token = resources::grant_token(&next.grant).unwrap();
        assert!(approve_resources(
            config,
            &path,
            "native-test",
            &digest,
            hedit_broker::state::Approval {
                command: "native-test.run".into(),
                token,
                scope: None,
                endpoints: vec![],
                tools: BTreeMap::from([("formatter".into(), image)])
            }
        )
        .is_err());
    }
}
