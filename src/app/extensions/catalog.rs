//! Local manifest discovery and atomic changes from the Extensions manager.
use super::manifest::{
    self, Contribution, MAX_DIRECTORY_ENTRIES, MAX_MANIFESTS, MAX_MANIFEST_BYTES,
};
use hedit::projects;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

const DISABLED_FILE: &str = "extensions-disabled.json";
const MAX_DISABLED_BYTES: u64 = 16 * 1024;
const MAX_DISABLED: usize = 256;

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
}

pub(super) struct ManifestStatus {
    pub(super) path: PathBuf,
    pub(super) details: Result<Details, String>,
}

pub(super) struct Details {
    pub(super) id: String,
    pub(super) commands: Vec<(String, String)>,
    pub(super) enabled: bool,
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
            let details = Details {
                id: manifest.id.clone(),
                commands: manifest.command_labels(),
                enabled: !catalog.view.state_unreadable && !disabled.contains(&manifest.id),
            };
            if details.enabled {
                catalog.contributions.extend(manifest.into_contributions());
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

#[derive(Clone)]
pub(super) struct ImportPreview {
    pub(super) source: PathBuf,
    pub(super) id: String,
    pub(super) commands: Vec<(String, String)>,
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
