//! Local manifest discovery and atomic changes from the Extensions manager.
use super::manifest::{
    self, Contribution, LuaPlugin, MAX_DIRECTORY_ENTRIES, MAX_MANIFESTS, MAX_MANIFEST_BYTES,
};
use hedit::projects;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

const DISABLED_FILE: &str = "extensions-disabled.json";
const MAX_DISABLED_BYTES: u64 = 16 * 1024;
const MAX_DISABLED: usize = 256;
const GRANTS_FILE: &str = "extensions-lua-grants.json";
const MAX_GRANTS_BYTES: u64 = 32 * 1024;
const MAX_GRANTS: usize = MAX_MANIFESTS * 2;

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
}

pub(super) struct Details {
    pub(super) id: String,
    pub(super) commands: Vec<(String, String)>,
    pub(super) enabled: bool,
    pub(super) lua: Option<LuaStatus>,
}

pub(super) struct LuaStatus {
    pub(super) digest: String,
    pub(super) approved: bool,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Grant {
    id: String,
    digest: String,
    // A fresh instance also invalidates an in-flight invocation after revoke
    // followed by approval of the exact same manifest.
    token: [u8; 16],
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

fn disabled(config: &Path) -> Result<BTreeSet<String>, String> {
    let path = config.join(DISABLED_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.to_string()),
    }
    let ids: BTreeSet<String> = projects::read_json(&path, MAX_DISABLED_BYTES)?;
    if ids.len() > MAX_DISABLED || ids.iter().any(|id| !manifest::valid_id(id)) {
        return Err("Invalid disabled extension IDs.".into());
    }
    Ok(ids)
}

fn lua_grants(config: &Path) -> Result<BTreeMap<String, Grant>, String> {
    let path = config.join(GRANTS_FILE);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err("The Lua approval file must be a regular file.".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.to_string()),
    }
    let records: Vec<Grant> = projects::read_json(&path, MAX_GRANTS_BYTES)?;
    if records.len() > MAX_GRANTS {
        return Err("Too many Lua approvals.".into());
    }
    let mut grants = BTreeMap::new();
    for record in records {
        if !manifest::valid_id(&record.id)
            || record.digest.len() != 64
            || !record
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || grants.insert(record.id.clone(), record).is_some()
        {
            return Err("Invalid Lua approvals.".into());
        }
    }
    Ok(grants)
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
    let grants = match lua_grants(config) {
        Ok(grants) => grants,
        Err(error) => {
            catalog.view.grants_unreadable = true;
            catalog.view.notices.push(format!(
                "Lua plugins are inactive. Could not read {GRANTS_FILE}: {error}"
            ));
            BTreeMap::new()
        }
    };
    let directory = config.join("extensions");
    let paths = match manifest::paths(&directory) {
        Ok((paths, _)) => paths,
        Err(error) => {
            catalog
                .view
                .notices
                .push(format!("Extensions in {}: {error}", directory.display()));
            return catalog;
        }
    };
    if paths.len() > MAX_MANIFESTS {
        catalog.view.notices.push(format!(
            "Extensions in {}: only the first {MAX_MANIFESTS} manifests by filename were read.",
            directory.display()
        ));
    }
    let mut ids = BTreeSet::new();
    for path in paths.into_iter().take(MAX_MANIFESTS) {
        let details = manifest::read_manifest(&path).and_then(|manifest| {
            if !ids.insert(manifest.id.clone()) {
                return Err(format!("duplicate extension ID {:?}.", manifest.id));
            }
            let grant = manifest.lua_digest.as_ref().and_then(|digest| {
                grants
                    .get(&manifest.id)
                    .filter(|grant| &grant.digest == digest)
            });
            let details = Details {
                id: manifest.id.clone(),
                commands: manifest.command_labels(),
                enabled: !catalog.view.state_unreadable && !disabled.contains(&manifest.id),
                lua: manifest.lua_digest.as_ref().map(|digest| LuaStatus {
                    digest: digest.clone(),
                    approved: grant.is_some(),
                }),
            };
            if details.enabled && (details.lua.is_none() || grant.is_some()) {
                catalog
                    .contributions
                    .extend(manifest.into_contributions(path.clone(), grant.cloned()));
            }
            Ok(details)
        });
        catalog.view.files.push(ManifestStatus { path, details });
    }
    catalog
}

fn change_lock(config: &Path) -> Result<File, String> {
    fs::create_dir_all(config).map_err(|error| error.to_string())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config.join("extensions.lock"))
        .map_err(|error| error.to_string())?;
    file.try_lock()
        .map_err(|error| format!("Could not lock extension changes: {error}. Try again."))?;
    Ok(file)
}

pub(super) fn set_enabled(config: &Path, id: &str, enabled: bool) -> Result<Catalog, String> {
    let _lock = change_lock(config)?;
    let mut ids = disabled(config)?;
    let installed = read(config)
        .view
        .files
        .iter()
        .any(|entry| entry.details.as_ref().is_ok_and(|details| details.id == id));
    if !installed {
        return Err("The extension changed on disk. Reload the list and try again.".into());
    }
    let changed = if enabled {
        ids.remove(id)
    } else {
        if ids.len() >= MAX_DISABLED && !ids.contains(id) {
            return Err(format!(
                "At most {MAX_DISABLED} extension IDs may be disabled."
            ));
        }
        ids.insert(id.to_owned())
    };
    if changed {
        projects::atomic_json(&config.join(DISABLED_FILE), &ids)?;
    }
    Ok(read(config))
}

pub(super) fn set_lua_approved(
    config: &Path,
    id: &str,
    digest: &str,
    approved: bool,
) -> Result<Catalog, String> {
    let _lock = change_lock(config)?;
    let mut grants = lua_grants(config)?;
    let current = read(config);
    let lua_manifests = current
        .view
        .files
        .iter()
        .filter_map(|entry| {
            let details = entry.details.as_ref().ok()?;
            Some((details.id.as_str(), details.lua.as_ref()?.digest.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    if lua_manifests.get(id).copied() != Some(digest) {
        return Err(
            "The plugin changed on disk. Reload extensions before changing approval.".into(),
        );
    }
    let changed = if approved {
        if grants.get(id).is_some_and(|grant| grant.digest == digest) {
            false
        } else {
            // Discard approvals for removed or superseded copies, making space
            // for future installs without an unbounded history of old grants.
            grants.retain(|id, grant| {
                lua_manifests.get(id.as_str()).copied() == Some(grant.digest.as_str())
            });
            let mut token = [0; 16];
            getrandom::fill(&mut token).map_err(|_| "Could not generate a Lua approval token.")?;
            grants.insert(
                id.to_owned(),
                Grant {
                    id: id.to_owned(),
                    digest: digest.to_owned(),
                    token,
                },
            );
            true
        }
    } else {
        grants.remove(id).is_some()
    };
    if changed {
        projects::atomic_json(
            &config.join(GRANTS_FILE),
            &grants.into_values().collect::<Vec<_>>(),
        )?;
    }
    Ok(read(config))
}

/// Check the installed bytes and current permissions. Keep the returned lock
/// through the resulting editor change so a concurrent revoke cannot interleave.
pub(super) fn lua_authorization(config: &Path, plugin: &LuaPlugin) -> Option<File> {
    if plugin.installed_path.parent()? != config.join("extensions") {
        return None;
    }
    let lock = change_lock(config).ok()?;
    let grant = &plugin.grant;
    if disabled(config).ok()?.contains(&grant.id)
        || lua_grants(config).ok()?.get(&grant.id) != Some(grant.as_ref())
        || !fs::symlink_metadata(&plugin.installed_path).ok()?.is_file()
    {
        return None;
    }
    let installed = projects::read_text(&plugin.installed_path, MAX_MANIFEST_BYTES).ok()?;
    (manifest::digest(&installed) == grant.digest).then_some(lock)
}

#[derive(Clone)]
pub(super) struct ImportPreview {
    pub(super) source: PathBuf,
    pub(super) id: String,
    pub(super) commands: Vec<(String, String)>,
    pub(super) lua: bool,
    text: String,
}

impl ImportPreview {
    pub(super) fn open(source: PathBuf) -> Result<Self, String> {
        let text = projects::read_text(&source, MAX_MANIFEST_BYTES)
            .map_err(|error| format!("{}: {error}", source.display()))?;
        let manifest = manifest::parse_manifest(&text)
            .map_err(|error| format!("{}: {error}", source.display()))?;
        Ok(Self {
            source,
            commands: manifest.command_labels(),
            lua: manifest.lua_digest.is_some(),
            id: manifest.id,
            text,
        })
    }

    pub(super) fn install(&self, config: &Path) -> Result<Catalog, String> {
        let _lock = change_lock(config)?;
        let directory = config.join("extensions");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let (paths, count) = manifest::paths(&directory).map_err(|error| error.to_string())?;
        if count >= MAX_DIRECTORY_ENTRIES {
            return Err(format!(
                "The extensions directory can contain at most {MAX_DIRECTORY_ENTRIES} entries."
            ));
        }
        if paths.len() >= MAX_MANIFESTS {
            return Err(format!(
                "Install supports at most {MAX_MANIFESTS} manifests."
            ));
        }
        if paths
            .iter()
            .any(|path| manifest::read_manifest(path).is_ok_and(|existing| existing.id == self.id))
        {
            return Err(format!("Extension {:?} is already installed.", self.id));
        }
        let destination = directory.join(format!("{}.json", self.id));
        let mut temp =
            tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
        temp.write_all(self.text.as_bytes())
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        temp.persist_noclobber(destination).map_err(|failure| {
            if failure.error.kind() == io::ErrorKind::AlreadyExists {
                format!("A manifest file for {:?} already exists.", self.id)
            } else {
                failure.error.to_string()
            }
        })?;
        Ok(read(config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let installed = preview.install(config.path()).unwrap();
        assert!(installed.contributions.is_empty());
        let state = installed.view.files[0]
            .details
            .as_ref()
            .unwrap()
            .lua
            .as_ref()
            .unwrap();
        assert!(!state.approved);
        assert_eq!(state.digest, manifest::digest(&original));
        assert!(!config.path().join(GRANTS_FILE).exists());
    }

    #[test]
    fn lua_approval_is_bound_to_plugin_id_and_manifest_bytes() {
        let config = tempfile::tempdir().unwrap();
        let (installed, original_digest) = lua_install(config.path());
        assert!(read(config.path()).contributions.is_empty());
        assert!(set_lua_approved(config.path(), "another", &original_digest, true).is_err());
        let approved = set_lua_approved(config.path(), "lua", &original_digest, true).unwrap();
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
        assert!(set_lua_approved(config.path(), "lua", &original_digest, true).is_err());

        let new_digest = manifest::digest(&changed);
        let approved = set_lua_approved(config.path(), "lua", &new_digest, true).unwrap();
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
        let approved = set_lua_approved(config.path(), "lua", &digest, true).unwrap();
        let invocation = authorized_plugin(&approved);
        let guard = lua_authorization(config.path(), &invocation).unwrap();
        assert!(set_lua_approved(config.path(), "lua", &digest, false).is_err());
        drop(guard);

        assert!(set_lua_approved(config.path(), "lua", &digest, false)
            .unwrap()
            .contributions
            .is_empty());
        assert!(lua_authorization(config.path(), &invocation).is_none());
        let approved_again = set_lua_approved(config.path(), "lua", &digest, true).unwrap();
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
        let approved = set_lua_approved(config.path(), "lua", &digest, true).unwrap();
        let invocation = authorized_plugin(&approved);
        let path = config.path().join(GRANTS_FILE);
        let valid = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(lua_authorization(config.path(), &invocation).is_none());
        let duplicate = format!(
            "[{},{}]",
            &valid.trim()[1..valid.trim().len() - 1],
            &valid.trim()[1..valid.trim().len() - 1]
        );
        for corrupt in ["{", &duplicate] {
            fs::write(&path, corrupt).unwrap();
            let catalog = read(config.path());
            assert!(catalog.view.grants_unreadable);
            assert_eq!(catalog.contributions.len(), 1);
            assert_eq!(catalog.contributions[0].id, "simple.insert");
            assert!(lua_authorization(config.path(), &invocation).is_none());
            assert!(set_lua_approved(config.path(), "lua", &digest, true).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), corrupt);
        }
    }

    #[test]
    fn import_installs_the_reviewed_content_and_refuses_duplicates() {
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
        let catalog = preview.install(config.path()).unwrap();
        assert_eq!(catalog.contributions[0].title, "Original title");
        let installed = config.path().join("extensions/reviewed.json");
        assert_eq!(fs::read_to_string(&installed).unwrap(), original);
        let updated = ImportPreview::open(source).unwrap();
        assert!(updated.install(config.path()).is_err());
        assert_eq!(fs::read_to_string(installed).unwrap(), original);
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
        assert!(preview.install(config.path()).is_err());
        assert!(!dir.join("new.json").exists());
        assert!(read(config.path()).view.notices.is_empty());
    }
}
