//! Local manifest discovery and atomic changes from the Extensions manager.
mod lifecycle;
mod permissions;
mod state;
pub(super) use state::{read_state, write_state};

use super::manifest::{self, Contribution, ExtensionAction, LuaPlugin, MAX_MANIFESTS};
use hedit::projects;
use lifecycle::{extensions_dir, inspect_manifest, FileIdentity};
pub(super) use lifecycle::{
    reset_unreadable_approvals, uninstall, ImportPreview, InstallChange, InstallReview,
    InstalledManifest,
};
use permissions::{
    advance_one_time_epoch, change_lock, disabled, invalidate_lua_grants, Grants, DISABLED_FILE,
    GRANTS_FILE,
};
pub(super) use permissions::{Grant, OneTimeApproval};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct Catalog {
    pub(super) contributions: Vec<Contribution>,
    pub(super) view: View,
}

#[derive(Default)]
pub(super) struct View {
    pub(super) files: Vec<ManifestStatus>,
    pub(super) notices: Vec<String>,
    pub(super) state_unreadable: bool,
    pub(super) grants_unreadable: bool,
}

pub(super) struct ManifestStatus {
    pub(super) path: PathBuf,
    pub(super) details: Result<Details, String>,
    pub(super) installed: Option<InstalledManifest>,
}

pub(super) struct Details {
    pub(super) id: String,
    pub(super) commands: Vec<(String, String)>,
    pub(super) enabled: bool,
    pub(super) lua: Option<LuaStatus>,
}

pub(super) struct LuaStatus {
    pub(super) sdk: bool,
    pub(super) digest: String,
    pub(super) commands: Vec<LuaCommandStatus>,
}

pub(super) struct LuaCommandStatus {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) approved: bool,
    pub(super) storage: bool,
}

impl View {
    pub(super) fn startup_errors(&self) -> Vec<String> {
        let mut errors = self.notices.clone();
        errors.extend(self.files.iter().filter_map(|entry| {
            let error = entry.details.as_ref().err()?;
            Some(format!("Extension {}: {error}", entry.path.display()))
        }));
        errors
    }
}

struct ScannedManifest {
    path: PathBuf,
    manifest: Result<manifest::Manifest, String>,
    installed: Option<InstalledManifest>,
}

fn scan(config: &Path) -> Result<(bool, Vec<ScannedManifest>), String> {
    let directory = extensions_dir(config)?;
    let (paths, _) = manifest::paths(&directory)
        .map_err(|error| format!("Extensions in {}: {error}", directory.display()))?;
    let over_limit = paths.len() > MAX_MANIFESTS;
    let entries = paths
        .into_iter()
        .take(MAX_MANIFESTS)
        .map(|path| {
            let (manifest, installed) = inspect_manifest(&path, Ok);
            ScannedManifest {
                path,
                manifest,
                installed,
            }
        })
        .collect();
    Ok((over_limit, entries))
}

pub(super) fn read(config: &Path) -> Catalog {
    let mut catalog = Catalog::default();
    let disabled = match disabled(config) {
        Ok(disabled) => disabled,
        Err(error) => {
            catalog.view.state_unreadable = true;
            catalog.view.notices.push(format!(
                "Extensions are inactive. Could not read {DISABLED_FILE}: {error}"
            ));
            BTreeSet::new()
        }
    };
    let grants = match Grants::read(config) {
        Ok(grants) => grants,
        Err(error) => {
            catalog.view.grants_unreadable = true;
            catalog.view.notices.push(format!(
                "Lua plugins are inactive. Could not read {GRANTS_FILE}: {error}"
            ));
            Grants::default()
        }
    };
    let (over_limit, entries) = match scan(config) {
        Ok(scan) => scan,
        Err(error) => {
            catalog.view.notices.push(error);
            return catalog;
        }
    };
    if over_limit {
        catalog.view.notices.push(format!(
            "Only the first {MAX_MANIFESTS} manifests by filename were read. Lua commands stay inactive until the number of manifests is within that limit."
        ));
    }
    let mut id_counts = BTreeMap::new();
    for manifest in entries
        .iter()
        .filter_map(|entry| entry.manifest.as_ref().ok())
    {
        *id_counts.entry(manifest.id.clone()).or_insert(0) += 1;
    }
    let mut ids = BTreeSet::new();
    for ScannedManifest {
        path,
        manifest,
        installed,
    } in entries
    {
        let details = manifest.and_then(|manifest| {
            let first = ids.insert(manifest.id.clone());
            let ambiguous_lua =
                manifest.lua_digest.is_some() && id_counts.get(&manifest.id) != Some(&1);
            if !first || ambiguous_lua {
                return Err(format!("duplicate extension ID {:?}.", manifest.id));
            }
            let mut matched = BTreeMap::new();
            let lua = manifest.lua_digest.as_ref().map(|digest| {
                let commands = manifest
                    .lua_command_labels()
                    .map(|(id, title)| {
                        let grant = grants.commands.get(&id).filter(|grant| {
                            !over_limit
                                && installed.as_ref().is_some_and(|file| grant.matches(file))
                        });
                        if let Some(grant) = grant {
                            matched.insert(id.clone(), grant.clone());
                        }
                        let storage = manifest.uses_storage(&id);
                        LuaCommandStatus {
                            id,
                            title: title.to_owned(),
                            approved: grant.is_some(),
                            storage,
                        }
                    })
                    .collect();
                LuaStatus {
                    sdk: manifest.sdk,
                    digest: digest.clone(),
                    commands,
                }
            });
            let details = Details {
                id: manifest.id.clone(),
                commands: manifest.command_labels(),
                enabled: !catalog.view.state_unreadable && !disabled.contains(&manifest.id),
                lua,
            };
            if details.enabled {
                catalog
                    .contributions
                    .extend(manifest.into_contributions(path.clone(), matched));
            }
            Ok(details)
        });
        catalog.view.files.push(ManifestStatus {
            path,
            details,
            installed,
        });
    }
    catalog
}

pub(super) fn set_enabled(config: &Path, id: &str, enabled: bool) -> Result<Catalog, String> {
    let _lock = change_lock(config)?;
    let mut ids = disabled(config)?;
    let current = read(config);
    let (entry, details) = current
        .view
        .files
        .iter()
        .find_map(|entry| {
            let details = entry
                .details
                .as_ref()
                .ok()
                .filter(|details| details.id == id)?;
            Some((entry, details))
        })
        .ok_or("The extension changed on disk. Reload the list and try again.")?;
    let changed = if enabled {
        ids.remove(id)
    } else {
        if ids.len() >= permissions::MAX_DISABLED && !ids.contains(id) {
            return Err(format!(
                "At most {} extension IDs may be disabled.",
                permissions::MAX_DISABLED
            ));
        }
        ids.insert(id.to_owned())
    };
    if changed {
        if details.lua.is_some() {
            // A disable/enable round trip never revives an in-flight proposal.
            invalidate_lua_grants(config, Some(id), &entry.path)
                .map_err(|error| format!("Could not clear Lua approvals: {error}"))?;
        }
        projects::atomic_json(&config.join(DISABLED_FILE), &ids)?;
    }
    Ok(read(config))
}

pub(super) fn set_lua_approved(
    config: &Path,
    path: &Path,
    id: &str,
    command: &str,
    digest: &str,
    approved: bool,
) -> Result<Catalog, String> {
    let lock = change_lock(config)?;
    let (installed, _) = current_lua_command(config, path, id, command)?;
    if installed.digest != digest {
        return Err("The Lua manifest changed. Reload extensions and review it again.".into());
    }
    if approved && disabled(config)?.contains(id) {
        return Err("Enable this extension before approving its Lua commands.".into());
    }
    let mut grants = Grants::read(config)?;
    let changed = if approved {
        if grants
            .commands
            .get(command)
            .is_some_and(|grant| grant.matches(&installed))
        {
            false
        } else {
            let current = read(config);
            let retained: BTreeSet<_> = current
                .view
                .files
                .iter()
                .filter_map(|entry| entry.details.as_ref().ok()?.lua.as_ref())
                .flat_map(|lua| lua.commands.iter())
                .filter(|command| command.approved)
                .map(|command| command.id.as_str())
                .collect();
            grants
                .commands
                .retain(|id, _| retained.contains(id.as_str()));
            grants.commands.insert(
                command.to_owned(),
                Grant::new(command.to_owned(), &installed)?,
            );
            true
        }
    } else {
        advance_one_time_epoch(&lock)?;
        grants.commands.remove(command).is_some()
    };
    if changed {
        grants.write(config)?;
    }
    Ok(read(config))
}

pub(super) fn prepare_one_time(
    config: &Path,
    path: &Path,
    id: &str,
    command: &str,
    digest: &str,
) -> Result<Contribution, String> {
    let lock = change_lock(config)?;
    let (installed, manifest) = current_lua_command(config, path, id, command)?;
    if installed.digest != digest {
        return Err("The Lua manifest changed. Reload extensions and review it again.".into());
    }
    if disabled(config)?.contains(id) {
        return Err("Enable this extension before allowing its command to run.".into());
    }
    let grants = Grants::read(config)?;
    if grants
        .commands
        .get(command)
        .is_some_and(|grant| grant.matches(&installed))
    {
        return Err("This command already has approval. Reload extensions to use it.".into());
    }

    let permit = Grant::new(command.to_owned(), &installed)?;
    let mut contribution = manifest
        .into_contributions(
            path.to_owned(),
            BTreeMap::from([(command.to_owned(), permit)]),
        )
        .find(|candidate| candidate.id == command)
        .ok_or("The Lua command changed. Review it again.")?;
    let ExtensionAction::Lua(plugin) = &mut contribution.action else {
        return Err("This command does not use Lua.".into());
    };
    if plugin.storage {
        return Err("Private storage requires persistent command approval.".into());
    }
    plugin.one_time = Some(Arc::new(OneTimeApproval::new(config, &lock)?));
    Ok(contribution)
}

fn current_lua_command(
    config: &Path,
    path: &Path,
    id: &str,
    command: &str,
) -> Result<(InstalledManifest, manifest::Manifest), String> {
    let (over_limit, entries) = scan(config)?;
    if over_limit {
        return Err(format!(
            "Keep at most {MAX_MANIFESTS} manifests to use Lua commands."
        ));
    }
    let mut matching = entries.into_iter().filter_map(|entry| {
        let manifest = entry.manifest.ok().filter(|manifest| manifest.id == id)?;
        Some((manifest, entry.installed?, entry.path))
    });
    let (manifest, installed, actual) = matching
        .next()
        .ok_or("The Lua plugin is no longer installed.")?;
    if matching.next().is_some() {
        return Err("Resolve the duplicate extension ID before approving Lua commands.".into());
    }
    if actual != path
        || !manifest
            .lua_command_labels()
            .any(|(name, _)| name == command)
    {
        return Err("The Lua command changed. Reload extensions and review it again.".into());
    }
    Ok((installed, manifest))
}

/// Let focus refresh drop obsolete queued approvals before showing the palette.
pub(super) fn one_time_current(config: &Path, catalog: &Catalog, command: &Contribution) -> bool {
    let ExtensionAction::Lua(plugin) = &command.action else {
        return false;
    };
    let Some(approval) = &plugin.one_time else {
        return false;
    };
    !catalog.view.grants_unreadable
        && !catalog.view.state_unreadable
        && !catalog
            .contributions
            .iter()
            .any(|existing| existing.id == command.id)
        && approval.policy_unchanged(config)
        && catalog.view.files.iter().any(|entry| {
            entry.path == plugin.installed_path
                && entry.details.as_ref().is_ok_and(|details| details.enabled)
                && entry
                    .installed
                    .as_ref()
                    .is_some_and(|installed| plugin.grant.matches(installed))
        })
}

/// Keep the lock through launch or edit application so a revoke cannot interleave.
pub(super) fn lua_authorization(config: &Path, plugin: &LuaPlugin) -> Option<File> {
    let lock = change_lock(config).ok()?;
    let grant = &plugin.grant;
    let id = grant.command.split_once('.')?.0;
    if disabled(config).ok()?.contains(id) {
        return None;
    }
    let grants = Grants::read(config).ok()?;
    let permitted = match &plugin.one_time {
        Some(approval) => approval.authorizes(config, &lock),
        None => grants.commands.get(&grant.command) == Some(grant.as_ref()),
    };
    if !permitted {
        return None;
    }
    let (installed, _) =
        current_lua_command(config, &plugin.installed_path, id, &grant.command).ok()?;
    grant.matches(&installed).then_some(lock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::extensions::manifest::MAX_DIRECTORY_ENTRIES;
    use std::fs;

    const COMMAND: &str = "lua.transform";

    fn set_approval(config: &Path, digest: &str, approved: bool) -> Result<Catalog, String> {
        set_lua_approved(
            config,
            &config.join("extensions/lua.json"),
            "lua",
            COMMAND,
            digest,
            approved,
        )
    }

    fn manifest(id: &str, title: &str) -> String {
        serde_json::json!({
            "api_version": 1,
            "id": id,
            "commands": [{
                "id": "insert",
                "title": title,
                "action": {"kind": "insert_snippet", "body": "example$0"}
            }]
        })
        .to_string()
    }

    fn lua_manifest(id: &str, source: &str) -> String {
        serde_json::json!({
            "api_version": 2,
            "id": id,
            "permissions": ["selection.read", "selection.replace"],
            "scripts": {"transform.lua": source},
            "commands": [{
                "id": "transform",
                "title": "Transform selection",
                "action": {"kind": "lua", "source": "transform.lua"}
            }]
        })
        .to_string()
    }

    fn lua_install(config: &Path) -> (PathBuf, String) {
        let dir = config.join("extensions");
        fs::create_dir_all(&dir).unwrap();
        let installed = dir.join("lua.json");
        let source = lua_manifest("lua", "return function(s) return s end");
        fs::write(&installed, &source).unwrap();
        (installed, manifest::digest(&source))
    }

    fn authorized_plugin(catalog: &Catalog) -> LuaPlugin {
        let super::manifest::ExtensionAction::Lua(plugin) = &catalog.contributions[0].action else {
            panic!("approved Lua command was not loaded");
        };
        plugin.clone()
    }

    fn once(config: &Path, path: &Path, digest: &str) -> LuaPlugin {
        let candidate = prepare_one_time(config, path, "lua", COMMAND, digest).unwrap();
        let ExtensionAction::Lua(plugin) = candidate.action else {
            panic!("expected Lua command");
        };
        plugin
    }

    #[test]
    fn one_time_approval_is_exact_ephemeral_and_claimed_once() {
        let config = tempfile::tempdir().unwrap();
        let (path, digest) = lua_install(config.path());
        assert!(prepare_one_time(config.path(), &path, "lua", COMMAND, "wrong").is_err());
        let plugin = once(config.path(), &path, &digest);
        assert!(!config.path().join(GRANTS_FILE).exists());
        assert!(read(config.path()).contributions.is_empty());
        assert!(plugin.is_one_time());
        assert!(plugin.approval_id().is_none());
        assert!(lua_authorization(config.path(), &plugin).is_none());

        let claim = plugin.one_time.as_ref().unwrap();
        assert!(claim.claim());
        let clone = plugin.clone();
        assert!(!clone.one_time.as_ref().unwrap().claim());
        assert!(lua_authorization(config.path(), &plugin).is_some());
        assert!(lua_authorization(config.path(), &plugin).is_some());

        // A revoke invalidates the in-flight invocation even if there was
        // no persistent approval record for this command.
        set_approval(config.path(), &digest, false).unwrap();
        assert!(lua_authorization(config.path(), &plugin).is_none());
        assert!(!config.path().join(GRANTS_FILE).exists());
    }

    #[test]
    fn concurrent_invocations_can_claim_a_one_time_approval_only_once() {
        let config = tempfile::tempdir().unwrap();
        let (path, digest) = lua_install(config.path());
        let plugin = once(config.path(), &path, &digest);
        let approval = plugin.one_time.as_ref().unwrap();
        let barrier = std::sync::Barrier::new(16);
        let winners = std::thread::scope(|scope| {
            let claims: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        approval.claim()
                    })
                })
                .collect();
            claims
                .into_iter()
                .map(|claim| usize::from(claim.join().unwrap()))
                .sum::<usize>()
        });
        assert_eq!(winners, 1);
        assert!(!approval.claim());
        assert!(lua_authorization(config.path(), &plugin).is_some());
    }

    #[test]
    fn one_time_approval_does_not_survive_disable_or_manifest_rollback() {
        let config = tempfile::tempdir().unwrap();
        let (path, digest) = lua_install(config.path());
        let plugin = once(config.path(), &path, &digest);
        set_enabled(config.path(), "lua", false).unwrap();
        set_enabled(config.path(), "lua", true).unwrap();
        assert!(plugin.one_time.as_ref().unwrap().claim());
        assert!(lua_authorization(config.path(), &plugin).is_none());

        let plugin = once(config.path(), &path, &digest);
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"changed manifest").unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, original).unwrap();
        assert!(plugin.one_time.as_ref().unwrap().claim());
        assert!(lua_authorization(config.path(), &plugin).is_none());
    }

    #[test]
    fn a_reset_revokes_one_time_approval_when_grant_storage_returns_to_absent() {
        let config = tempfile::tempdir().unwrap();
        let (path, digest) = lua_install(config.path());
        let original = once(config.path(), &path, &digest);
        let state = config.path().join(GRANTS_FILE);

        fs::write(&state, b"unreadable").unwrap();
        reset_unreadable_approvals(config.path()).unwrap();
        assert!(!state.exists());
        assert!(original.one_time.as_ref().unwrap().claim());
        assert!(lua_authorization(config.path(), &original).is_none());

        let after_reset = once(config.path(), &path, &digest);
        assert!(after_reset.one_time.as_ref().unwrap().claim());
        assert!(lua_authorization(config.path(), &after_reset).is_some());
    }

    #[test]
    fn lua_import_installs_reviewed_bytes_without_approval() {
        let src = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let source = src.path().join("plugin.json");
        let original = lua_manifest("lua", "return function(s) return s end");
        fs::write(&source, &original).unwrap();
        let preview = ImportPreview::open(source.clone()).unwrap();
        fs::write(
            &source,
            lua_manifest("lua", "return function() return 'new' end"),
        )
        .unwrap();

        assert!(preview.lua);
        let installed = preview
            .review(config.path())
            .unwrap()
            .install(config.path())
            .unwrap();
        assert!(installed.contributions.is_empty());
        let state = installed.view.files[0]
            .details
            .as_ref()
            .unwrap()
            .lua
            .as_ref()
            .unwrap();
        assert!(!state.commands[0].approved);
        assert_eq!(state.digest, manifest::digest(&original));
        assert!(!config.path().join(GRANTS_FILE).exists());
    }

    #[test]
    fn private_storage_is_scoped_to_approval_and_cleared_on_revocation() {
        let config = tempfile::tempdir().unwrap();
        let (path, _) = lua_install(config.path());
        let manifest=serde_json::json!({"api_version":3,"sdk_version":1,"id":"lua","permissions":["document.read","document.edit","storage.private"],"scripts":{"transform.lua":"return function(ctx) local t=hedit.transaction({});t.storage={count=1};return t end"},"commands":[{"id":"transform","title":"Counter","private_storage":true,"action":{"kind":"lua","source":"transform.lua"}}]}).to_string();
        fs::write(&path, &manifest).unwrap();
        let digest = manifest::digest(&manifest);
        let plugin = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        assert!(prepare_one_time(config.path(), &path, "lua", COMMAND, &digest).is_err());
        let authority = lua_authorization(config.path(), &plugin).unwrap();
        assert!(read_state(config.path(), &plugin).unwrap().0.is_empty());
        let values = crate::plugin_runtime::sdk::Storage(
            [("count".into(), hedit::plugin_options::Value::Integer(7))].into(),
        );
        write_state(config.path(), &plugin, &values).unwrap();
        assert_eq!(read_state(config.path(), &plugin).unwrap(), values);
        drop(authority);
        set_approval(config.path(), &digest, false).unwrap();
        assert_eq!(
            fs::read_dir(config.path().join("extensions-state"))
                .unwrap()
                .count(),
            0
        );
        let fresh = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        assert_ne!(plugin.approval_id(), fresh.approval_id());
        assert!(read_state(config.path(), &fresh).unwrap().0.is_empty());
        assert!(lua_authorization(config.path(), &plugin).is_none());
    }

    #[test]
    fn private_storage_commands_cannot_read_sibling_buckets_or_unsafe_files() {
        let config = tempfile::tempdir().unwrap();
        let (path, _) = lua_install(config.path());
        let manifest = serde_json::json!({
            "api_version":3,"sdk_version":1,"id":"lua",
            "permissions":["document.read","document.edit","storage.private"],
            "scripts":{"transform.lua":"return function(ctx) return hedit.transaction({}) end"},
            "commands":[
                {"id":"transform","title":"One","private_storage":true,"action":{"kind":"lua","source":"transform.lua"}},
                {"id":"other","title":"Two","private_storage":true,"action":{"kind":"lua","source":"transform.lua"}}
            ]
        }).to_string();
        fs::write(&path, &manifest).unwrap();
        let digest = manifest::digest(&manifest);
        let first = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        let catalog =
            set_lua_approved(config.path(), &path, "lua", "lua.other", &digest, true).unwrap();
        let ExtensionAction::Lua(second) = &catalog
            .contributions
            .iter()
            .find(|c| c.id == "lua.other")
            .unwrap()
            .action
        else {
            unreachable!()
        };
        let values = crate::plugin_runtime::sdk::Storage(
            [(
                "secret".into(),
                hedit::plugin_options::Value::Text("private fixture".into()),
            )]
            .into(),
        );
        write_state(config.path(), &first, &values).unwrap();
        assert_eq!(read_state(config.path(), &first).unwrap(), values);
        assert!(read_state(config.path(), second).unwrap().0.is_empty());
        let bucket = config
            .path()
            .join("extensions-state")
            .join(format!("{}.json", first.approval_id().unwrap()));
        fs::write(
            &bucket,
            br#"{"version":1,"values":{"secret":"one","secret":"two"}}"#,
        )
        .unwrap();
        assert!(read_state(config.path(), &first).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            fs::remove_file(&bucket).unwrap();
            let outside = config.path().join("outside");
            fs::write(&outside, b"outside sentinel").unwrap();
            symlink(&outside, &bucket).unwrap();
            assert!(read_state(config.path(), &first).is_err());
            assert!(write_state(config.path(), &first, &values).is_err());
            assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");
            fs::remove_file(&bucket).unwrap();
            write_state(config.path(), &first, &values).unwrap();
            fs::set_permissions(&bucket, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(read_state(config.path(), &first).is_err());
        }
    }

    #[test]
    fn selection_grants_do_not_authorize_document_sdk_commands() {
        let config = tempfile::tempdir().unwrap();
        let (path, digest) = lua_install(config.path());
        let old = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        let manifest=serde_json::json!({"api_version":3,"sdk_version":1,"id":"lua","permissions":["document.read","document.edit"],"scripts":{"transform.lua":"return function(ctx) return hedit.replace(ctx.document.text) end"},"commands":[{"id":"transform","title":"Edit document","action":{"kind":"lua","source":"transform.lua"}}]}).to_string();
        fs::write(&path, &manifest).unwrap();
        assert!(lua_authorization(config.path(), &old).is_none());
        assert!(read(config.path()).contributions.is_empty());
        let new = authorized_plugin(
            &set_approval(config.path(), &manifest::digest(&manifest), true).unwrap(),
        );
        assert!(new.sdk);
        assert!(lua_authorization(config.path(), &new).is_some());
        assert!(lua_authorization(config.path(), &old).is_none());
    }

    #[test]
    fn lua_approval_is_bound_to_plugin_id_and_manifest_bytes() {
        let config = tempfile::tempdir().unwrap();
        let (installed, original_digest) = lua_install(config.path());
        assert!(read(config.path()).contributions.is_empty());
        assert!(set_lua_approved(
            config.path(),
            &installed,
            "another",
            COMMAND,
            &original_digest,
            true
        )
        .is_err());
        let approved = set_approval(config.path(), &original_digest, true).unwrap();
        let old = authorized_plugin(&approved);
        assert!(lua_authorization(config.path(), &old).is_some());
        assert!(lua_authorization(installed.parent().unwrap(), &old).is_none());
        assert!(
            lua_authorization(config.path(), &authorized_plugin(&read(config.path()))).is_some()
        );

        let changed = lua_manifest("lua", "return function(s) return s .. '!' end");
        fs::write(&installed, &changed).unwrap();
        assert!(lua_authorization(config.path(), &old).is_none());
        assert!(read(config.path()).contributions.is_empty());
        assert!(set_approval(config.path(), &original_digest, true).is_err());

        let new_digest = manifest::digest(&changed);
        let approved = set_approval(config.path(), &new_digest, true).unwrap();
        let new = authorized_plugin(&approved);
        assert!(lua_authorization(config.path(), &new).is_some());
        assert!(lua_authorization(config.path(), &old).is_none());
        fs::remove_file(installed).unwrap();
        assert!(lua_authorization(config.path(), &new).is_none());
    }

    #[test]
    fn lua_revocation_serializes_with_application_and_invalidates_old_invocations() {
        let config = tempfile::tempdir().unwrap();
        let (_, digest) = lua_install(config.path());
        let approved = set_approval(config.path(), &digest, true).unwrap();
        let invocation = authorized_plugin(&approved);
        let guard = lua_authorization(config.path(), &invocation).unwrap();
        assert!(set_approval(config.path(), &digest, false).is_err());
        drop(guard);

        assert!(set_approval(config.path(), &digest, false)
            .unwrap()
            .contributions
            .is_empty());
        assert!(lua_authorization(config.path(), &invocation).is_none());
        let approved_again = set_approval(config.path(), &digest, true).unwrap();
        let new = authorized_plugin(&approved_again);
        assert!(lua_authorization(config.path(), &new).is_some());
        assert!(lua_authorization(config.path(), &invocation).is_none());
        set_enabled(config.path(), "lua", false).unwrap();
        assert!(lua_authorization(config.path(), &new).is_none());
    }

    #[test]
    fn invalid_grant_storage_denies_lua_and_preserves_declarative_commands() {
        let config = tempfile::tempdir().unwrap();
        let (installed, digest) = lua_install(config.path());
        fs::write(
            installed.with_file_name("simple.json"),
            manifest("simple", "Simple"),
        )
        .unwrap();
        let approved = set_approval(config.path(), &digest, true).unwrap();
        let invocation = authorized_plugin(&approved);
        let path = config.path().join(GRANTS_FILE);
        let valid = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(lua_authorization(config.path(), &invocation).is_none());
        let mut duplicate: serde_json::Value = serde_json::from_str(&valid).unwrap();
        let repeated = duplicate["grants"][0].clone();
        duplicate["grants"].as_array_mut().unwrap().push(repeated);
        let duplicate = duplicate.to_string();
        for corrupt in ["{", &duplicate] {
            fs::write(&path, corrupt).unwrap();
            let catalog = read(config.path());
            assert!(catalog.view.grants_unreadable);
            assert_eq!(catalog.contributions.len(), 1);
            assert_eq!(catalog.contributions[0].id, "simple.insert");
            assert!(lua_authorization(config.path(), &invocation).is_none());
            assert!(set_approval(config.path(), &digest, true).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), corrupt);
        }
    }

    #[test]
    fn approvals_grant_only_the_reviewed_command() {
        let config = tempfile::tempdir().unwrap();
        let (installed, _) = lua_install(config.path());
        let mut plugin: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&installed).unwrap()).unwrap();
        plugin["commands"].as_array_mut().unwrap().extend([
            serde_json::json!({"id":"second","title":"Second","action":{"kind":"lua","source":"transform.lua"}}),
            serde_json::json!({"id":"snippet","title":"Snippet","action":{"kind":"insert_snippet","body":"hello"}}),
        ]);
        let plugin = plugin.to_string();
        fs::write(&installed, &plugin).unwrap();
        let digest = manifest::digest(&plugin);
        let cmds = |catalog: &Catalog| -> Vec<String> {
            catalog
                .contributions
                .iter()
                .map(|contribution| contribution.id.clone())
                .collect()
        };
        assert_eq!(cmds(&read(config.path())), ["lua.snippet"]);
        assert!(set_lua_approved(
            config.path(),
            &installed,
            "lua",
            "lua.snippet",
            &digest,
            true
        )
        .is_err());
        let approved = set_approval(config.path(), &digest, true).unwrap();
        assert_eq!(cmds(&approved), ["lua.transform", "lua.snippet"]);
        let transform = authorized_plugin(&approved);
        let both = set_lua_approved(
            config.path(),
            &installed,
            "lua",
            "lua.second",
            &digest,
            true,
        )
        .unwrap();
        assert_eq!(cmds(&both), ["lua.transform", "lua.second", "lua.snippet"]);
        let revoked = set_approval(config.path(), &digest, false).unwrap();
        assert_eq!(cmds(&revoked), ["lua.second", "lua.snippet"]);
        assert!(lua_authorization(config.path(), &transform).is_none());
        assert!(lua_authorization(config.path(), &authorized_plugin(&revoked)).is_some());
    }

    #[test]
    fn unversioned_grants_require_explicit_reset_before_command_approval() {
        let config = tempfile::tempdir().unwrap();
        let (_, digest) = lua_install(config.path());
        let file = config.path().join(GRANTS_FILE);
        let token = [1u8; 16];
        let unversioned =
            serde_json::json!([{"id": "lua", "digest": digest, "token": token}]).to_string();
        fs::write(&file, &unversioned).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let unapproved = read(config.path());
        assert!(unapproved.contributions.is_empty());
        assert!(unapproved.view.grants_unreadable);
        assert_eq!(fs::read_to_string(&file).unwrap(), unversioned);
        assert!(set_approval(config.path(), &digest, true).is_err());
        reset_unreadable_approvals(config.path()).unwrap();
        let approved = set_approval(config.path(), &digest, true).unwrap();
        let new: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(new["version"], 1);
        assert_eq!(new["grants"][0]["command"], COMMAND);
        assert_eq!(
            new["grants"][0]["capabilities"],
            serde_json::json!(["selection.read", "selection.replace"])
        );
        let invocation = authorized_plugin(&approved);
        assert!(lua_authorization(config.path(), &invocation).is_some());

        let mut tampered = new;
        tampered["grants"][0]["capabilities"] =
            serde_json::json!(["selection.read", "selection.read"]);
        fs::write(&file, tampered.to_string()).unwrap();
        assert!(read(config.path()).view.grants_unreadable);
        assert!(lua_authorization(config.path(), &invocation).is_none());
    }

    #[test]
    fn disable_and_reenable_invalidates_all_earlier_permission_tokens() {
        let config = tempfile::tempdir().unwrap();
        let (_, digest) = lua_install(config.path());
        let invocation = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        set_enabled(config.path(), "lua", false).unwrap();
        assert!(set_approval(config.path(), &digest, true).is_err());
        let enabled = set_enabled(config.path(), "lua", true).unwrap();
        assert!(enabled.contributions.is_empty());
        assert!(lua_authorization(config.path(), &invocation).is_none());
        let current = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        assert!(lua_authorization(config.path(), &current).is_some());
        assert!(lua_authorization(config.path(), &invocation).is_none());
    }

    #[test]
    fn moved_manifest_and_duplicate_ids_fail_closed_for_live_checks() {
        let config = tempfile::tempdir().unwrap();
        let (original, digest) = lua_install(config.path());
        let invocation = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        let duplicate = original.with_file_name("z-duplicate.json");
        fs::write(&duplicate, manifest("lua", "Same identity")).unwrap();
        let ambiguous = read(config.path());
        assert!(ambiguous.contributions.is_empty());
        assert!(ambiguous
            .view
            .files
            .iter()
            .all(|entry| entry.details.is_err()));
        assert!(lua_authorization(config.path(), &invocation).is_none());
        assert!(set_approval(config.path(), &digest, true).is_err());
        fs::remove_file(duplicate).unwrap();
        assert!(lua_authorization(config.path(), &invocation).is_some());

        let changed = original.with_file_name("moved.json");
        fs::rename(&original, &changed).unwrap();
        assert!(read(config.path()).contributions.is_empty());
        assert!(set_approval(config.path(), &digest, true).is_err());
        assert!(lua_authorization(config.path(), &invocation).is_none());
        let current =
            set_lua_approved(config.path(), &changed, "lua", COMMAND, &digest, true).unwrap();
        assert!(lua_authorization(config.path(), &authorized_plugin(&current)).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn exact_bytes_reinstalled_under_the_same_name_need_a_new_approval() {
        let config = tempfile::tempdir().unwrap();
        let (installed, digest) = lua_install(config.path());
        let invocation = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        let original = fs::read(&installed).unwrap();
        let previous = installed.with_file_name("previous.txt");
        fs::rename(&installed, &previous).unwrap();
        fs::write(&installed, original).unwrap();
        assert!(read(config.path()).contributions.is_empty());
        assert!(lua_authorization(config.path(), &invocation).is_none());
        let current = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        assert!(lua_authorization(config.path(), &current).is_some());
        assert!(lua_authorization(config.path(), &invocation).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_hardlink_control_files_do_not_authorize_execution() {
        use std::os::unix::fs::symlink;

        let config = tempfile::tempdir().unwrap();
        let (_, digest) = lua_install(config.path());
        let invocation = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        let grant_file = config.path().join(GRANTS_FILE);
        let alternate = config.path().join("alternate.json");
        fs::hard_link(&grant_file, &alternate).unwrap();
        assert!(read(config.path()).view.grants_unreadable);
        assert!(lua_authorization(config.path(), &invocation).is_none());
        fs::remove_file(&grant_file).unwrap();
        symlink(&alternate, &grant_file).unwrap();
        assert!(read(config.path()).view.grants_unreadable);
        assert!(lua_authorization(config.path(), &invocation).is_none());

        fs::remove_file(&grant_file).unwrap();
        fs::rename(&alternate, &grant_file).unwrap();
        let lock = config.path().join("extensions.lock");
        fs::remove_file(&lock).unwrap();
        let external = config.path().join("unrelated");
        fs::write(&external, "keep").unwrap();
        symlink(&external, &lock).unwrap();
        assert!(lua_authorization(config.path(), &invocation).is_none());
        assert!(set_approval(config.path(), &digest, false).is_err());
        assert_eq!(fs::read_to_string(&external).unwrap(), "keep");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn shared_writable_grant_storage_denies_earlier_approvals() {
        use std::os::unix::fs::PermissionsExt as _;

        let config = tempfile::tempdir().unwrap();
        let (_, digest) = lua_install(config.path());
        let invocation = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        let grants = config.path().join(GRANTS_FILE);
        fs::set_permissions(&grants, fs::Permissions::from_mode(0o660)).unwrap();
        assert!(read(config.path()).view.grants_unreadable);
        assert!(lua_authorization(config.path(), &invocation).is_none());
        assert!(set_approval(config.path(), &digest, true).is_err());
        assert_eq!(
            fs::metadata(grants).unwrap().permissions().mode() & 0o777,
            0o660
        );
    }

    #[test]
    fn excessive_discovery_never_uses_an_incomplete_view_to_grant_lua() {
        let config = tempfile::tempdir().unwrap();
        let (installed, digest) = lua_install(config.path());
        let invocation = authorized_plugin(&set_approval(config.path(), &digest, true).unwrap());
        for index in 0..MAX_MANIFESTS {
            let id = format!("z-{index}");
            fs::write(
                installed.with_file_name(format!("{id}.json")),
                manifest(&id, "Snippet"),
            )
            .unwrap();
        }
        let bounded = read(config.path());
        assert_eq!(bounded.contributions.len(), MAX_MANIFESTS - 1);
        assert!(lua_authorization(config.path(), &invocation).is_none());
        assert!(set_approval(config.path(), &digest, true).is_err());
    }

    #[test]
    fn import_installs_the_reviewed_content_and_updates_after_review() {
        let source_dir = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("download.json");
        let original = manifest("reviewed", "Original title");
        fs::write(&source, &original).unwrap();
        let preview = ImportPreview::open(source.clone()).unwrap();
        assert_eq!(
            preview.commands,
            [("reviewed.insert".into(), "Original title".into())]
        );

        fs::write(&source, manifest("reviewed", "Changed title")).unwrap();
        let catalog = preview
            .review(config.path())
            .unwrap()
            .install(config.path())
            .unwrap();
        assert_eq!(catalog.contributions[0].title, "Original title");
        let installed = config.path().join("extensions/reviewed.json");
        assert_eq!(fs::read_to_string(&installed).unwrap(), original);
        let updated = ImportPreview::open(source)
            .unwrap()
            .review(config.path())
            .unwrap();
        assert!(matches!(updated.change, InstallChange::Update { .. }));
        assert_eq!(fs::read_to_string(&installed).unwrap(), original);
        assert_eq!(
            updated.install(config.path()).unwrap().contributions[0].title,
            "Changed title"
        );
    }

    #[test]
    fn extension_disable_persists_and_validation_errors_are_per_manifest() {
        let config = tempfile::tempdir().unwrap();
        let dir = config.path().join("extensions");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("good.json"), manifest("working", "Insert text")).unwrap();
        fs::write(dir.join("broken.json"), "{").unwrap();
        let initial = read(config.path());
        assert_eq!(initial.contributions.len(), 1);
        assert_eq!(initial.view.files.len(), 2);
        assert!(initial.view.files[0].details.is_err());

        assert!(set_enabled(config.path(), "working", false)
            .unwrap()
            .contributions
            .is_empty());
        let reloaded = read(config.path());
        assert!(reloaded.contributions.is_empty());
        let valid = reloaded.view.files[1].details.as_ref().unwrap();
        assert!(!valid.enabled);
        assert_eq!(valid.commands.len(), 1);
        assert_eq!(
            set_enabled(config.path(), "working", true)
                .unwrap()
                .contributions
                .len(),
            1
        );
    }

    #[test]
    fn corrupt_disabled_state_stays_on_disk_and_keeps_extensions_inactive() {
        let config = tempfile::tempdir().unwrap();
        let dir = config.path().join("extensions");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("good.json"), manifest("working", "Insert text")).unwrap();
        let state = config.path().join(DISABLED_FILE);
        fs::write(&state, "[").unwrap();
        let catalog = read(config.path());
        assert!(catalog.view.state_unreadable);
        assert!(catalog.contributions.is_empty());
        assert!(set_enabled(config.path(), "working", true).is_err());
        assert_eq!(fs::read_to_string(state).unwrap(), "[");
    }

    #[test]
    fn import_leaves_room_for_bounded_directory_discovery() {
        let config = tempfile::tempdir().unwrap();
        let dir = config.path().join("extensions");
        fs::create_dir(&dir).unwrap();
        for index in 0..MAX_DIRECTORY_ENTRIES {
            fs::write(dir.join(format!("{index}.txt")), "").unwrap();
        }
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("import.json");
        fs::write(&source, manifest("new", "New command")).unwrap();
        let preview = ImportPreview::open(source).unwrap();
        assert!(preview.review(config.path()).is_err());
        assert!(!dir.join("new.json").exists());
        assert!(read(config.path()).view.notices.is_empty());
    }
}
