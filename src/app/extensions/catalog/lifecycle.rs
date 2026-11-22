//! Review a snapshot before changing files in the user's extension directory.
use super::{
    change_lock, disabled, invalidate_lua_grants, read, Catalog, Grants, DISABLED_FILE, GRANTS_FILE,
};
use crate::app::extensions::manifest::{
    self, Manifest, MAX_DIRECTORY_ENTRIES, MAX_MANIFESTS, MAX_MANIFEST_BYTES,
};
use hedit::projects;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, Metadata, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::extensions) struct InstalledManifest {
    pub(in crate::app::extensions) path: PathBuf,
    pub(in crate::app::extensions) id: Option<String>,
    pub(super) digest: String,
    pub(super) identity: FileIdentity,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileIdentity {
    bytes: u64,
    revision: FileRevision,
}

// Keep both forms readable so copying user settings to another operating
// system does not render the protected approval state unreadable.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum FileRevision {
    Unix {
        device: u64,
        inode: u64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
    },
    Portable {
        modified: Option<std::time::Duration>,
    },
}

impl From<Metadata> for FileIdentity {
    fn from(metadata: Metadata) -> Self {
        #[cfg(unix)]
        let revision = {
            use std::os::unix::fs::MetadataExt;
            FileRevision::Unix {
                device: metadata.dev(),
                inode: metadata.ino(),
                changed_seconds: metadata.ctime(),
                changed_nanoseconds: metadata.ctime_nsec(),
            }
        };
        #[cfg(not(unix))]
        let revision = FileRevision::Portable {
            modified: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok()),
        };
        Self {
            bytes: metadata.len(),
            revision,
        }
    }
}

/// Parent directory is controlled by user settings. The extensions subdirectory
/// itself must be a real directory so managed operations never traverse a link.
pub(super) fn extensions_dir(config: &Path) -> Result<PathBuf, String> {
    let directory = config.join("extensions");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() => Ok(directory),
        Ok(_) => Err("The extensions folder must be a real directory.".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(directory),
        Err(error) => Err(error.to_string()),
    }
}

fn read_installed(path: &Path) -> Result<(Vec<u8>, FileIdentity), String> {
    fn snapshot(path: &Path) -> io::Result<(Vec<u8>, FileIdentity)> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use rustix::fs::OFlags;
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        if !fs::symlink_metadata(path)?.is_file() {
            return Err(io::Error::other("Choose a regular manifest file."));
        }
        let mut file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::other("Choose a regular manifest file."));
        }
        let identity = FileIdentity::from(metadata);
        let size_error = || {
            io::Error::other(format!(
                "Manifest exceeds the {MAX_MANIFEST_BYTES}-byte limit."
            ))
        };
        if identity.bytes > MAX_MANIFEST_BYTES {
            return Err(size_error());
        }
        let mut bytes = Vec::with_capacity(identity.bytes as usize);
        (&mut file)
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(size_error());
        }
        if FileIdentity::from(file.metadata()?) != identity || bytes.len() as u64 != identity.bytes
        {
            return Err(io::Error::other(
                "The manifest changed while it was being read.",
            ));
        }
        Ok((bytes, identity))
    }
    snapshot(path).map_err(|error| error.to_string())
}

pub(super) fn inspect_manifest<T>(
    path: &Path,
    process: impl FnOnce(Manifest) -> Result<T, String>,
) -> (Result<T, String>, Option<InstalledManifest>) {
    let (bytes, identity) = match read_installed(path) {
        Ok(snapshot) => snapshot,
        Err(error) => return (Err(error), None),
    };
    let digest = manifest::digest(&bytes);
    let manifest = std::str::from_utf8(&bytes)
        .map_err(|error| error.to_string())
        .and_then(manifest::parse_manifest);
    let reviewed = InstalledManifest {
        path: path.to_owned(),
        id: manifest.as_ref().ok().map(|manifest| manifest.id.clone()),
        digest,
        identity,
    };
    (manifest.and_then(process), Some(reviewed))
}

#[derive(Clone)]
pub(in crate::app::extensions) struct ImportPreview {
    pub(in crate::app::extensions) source: PathBuf,
    pub(in crate::app::extensions) id: String,
    pub(in crate::app::extensions) commands: Vec<(String, String)>,
    pub(in crate::app::extensions) lua: bool,
    pub(in crate::app::extensions) digest: String,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::extensions) enum InstallChange {
    New,
    Update {
        installed: InstalledManifest,
        previous_commands: Vec<(String, String)>,
    },
    Unchanged,
}

#[derive(Clone)]
pub(in crate::app::extensions) struct InstallReview {
    pub(in crate::app::extensions) preview: ImportPreview,
    pub(in crate::app::extensions) change: InstallChange,
}

impl ImportPreview {
    pub(in crate::app::extensions) fn open(source: PathBuf) -> Result<Self, String> {
        let text = projects::read_text(&source, MAX_MANIFEST_BYTES)
            .map_err(|error| format!("{}: {error}", source.display()))?;
        let manifest = manifest::parse_manifest(&text)
            .map_err(|error| format!("{}: {error}", source.display()))?;
        Ok(Self {
            source,
            commands: manifest.command_labels(),
            lua: manifest.lua_digest.is_some(),
            digest: manifest::digest(&text),
            id: manifest.id,
            text,
        })
    }

    pub(in crate::app::extensions) fn review(self, config: &Path) -> Result<InstallReview, String> {
        let change = self.change(config)?;
        Ok(InstallReview {
            preview: self,
            change,
        })
    }

    fn change(&self, config: &Path) -> Result<InstallChange, String> {
        let directory = extensions_dir(config)?;
        let (paths, count) = manifest::paths(&directory).map_err(|error| error.to_string())?;
        let mut matching = None;
        for path in &paths {
            let (parsed, installed) = inspect_manifest(path, Ok);
            if let (Ok(manifest), Some(installed)) = (parsed, installed) {
                if manifest.id == self.id {
                    if matching.is_some() {
                        return Err(format!(
                            "More than one installed manifest uses {:?}. Uninstall the conflicting copy first.",
                            self.id,
                        ));
                    }
                    matching = Some((manifest, installed));
                }
            }
        }
        if let Some((manifest, installed)) = matching {
            if installed.digest == self.digest {
                return Ok(InstallChange::Unchanged);
            }
            return Ok(InstallChange::Update {
                installed,
                previous_commands: manifest.command_labels(),
            });
        }
        let destination = directory.join(format!("{}.json", self.id));
        match fs::symlink_metadata(&destination) {
            Ok(_) => return Err(format!(
                "An installed file already uses {}. Uninstall that entry before installing this extension.",
                destination.file_name().unwrap_or_default().to_string_lossy(),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        if count >= MAX_DIRECTORY_ENTRIES || paths.len() >= MAX_MANIFESTS {
            return Err(format!(
                "The extensions folder is full. Uninstall an extension before installing another. hedit supports {MAX_MANIFESTS} manifests.",
            ));
        }
        Ok(InstallChange::New)
    }
}

impl InstallReview {
    pub(in crate::app::extensions) fn install(&self, config: &Path) -> Result<Catalog, String> {
        let _lock = change_lock(config)?;
        if self.preview.change(config)? != self.change {
            return Err(
                "The installed extension changed. Choose the file again to review it.".into(),
            );
        }
        let directory = extensions_dir(config)?;
        let destination = match &self.change {
            InstallChange::New => directory.join(format!("{}.json", self.preview.id)),
            InstallChange::Update { installed, .. } => installed.path.clone(),
            InstallChange::Unchanged => return Ok(read(config)),
        };

        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let mut temp =
            tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
        temp.write_all(self.preview.text.as_bytes())
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|error| error.to_string())?;

        // If an update is interrupted after revocation, the old copy still
        // needs a new approval. A replacement can never inherit consent.
        invalidate_lua_grants(config, Some(&self.preview.id), &destination)?;
        match &self.change {
            InstallChange::New => {
                let mut disabled_ids = disabled(config)?;
                if disabled_ids.remove(&self.preview.id) {
                    projects::atomic_json(&config.join(DISABLED_FILE), &disabled_ids)?;
                }
                temp.persist_noclobber(&destination).map_err(|failure| {
                    if failure.error.kind() == io::ErrorKind::AlreadyExists {
                        "An installed file appeared. Choose the file again to review it.".into()
                    } else {
                        failure.error.to_string()
                    }
                })?;
            }
            InstallChange::Update { installed, .. } => {
                verify_installed(installed)?;
                temp.persist(&destination)
                    .map_err(|failure| failure.error.to_string())?;
            }
            InstallChange::Unchanged => unreachable!(),
        }
        sync_directory(&directory)?;
        Ok(read(config))
    }
}

pub(in crate::app::extensions) fn uninstall(
    config: &Path,
    installed: &InstalledManifest,
) -> Result<Catalog, String> {
    let _lock = change_lock(config)?;
    let directory = extensions_dir(config)?;
    if installed.path.parent() != Some(directory.as_path())
        || installed
            .path
            .extension()
            .is_none_or(|extension| extension != "json")
    {
        return Err("Choose a manifest installed in the extensions folder.".into());
    }
    verify_installed(installed)?;
    invalidate_lua_grants(config, installed.id.as_deref(), &installed.path)?;
    verify_installed(installed)?;
    fs::remove_file(&installed.path).map_err(|error| error.to_string())?;
    sync_directory(&directory)?;
    Ok(read(config))
}

pub(in crate::app::extensions) fn reset_unreadable_approvals(
    config: &Path,
) -> Result<Catalog, String> {
    let _lock = change_lock(config)?;
    if Grants::read(config).is_ok() {
        return Err("Lua approvals changed. Reload extensions before resetting them.".into());
    }
    let state = config.join(GRANTS_FILE);
    let metadata = fs::symlink_metadata(&state).map_err(|error| error.to_string())?;
    if !metadata.is_file() && !metadata.is_symlink() {
        return Err(format!(
            "Move {} out of the configuration folder and reload extensions.",
            state.display()
        ));
    }
    let backup = tempfile::Builder::new()
        .prefix("lua-approvals-backup-")
        .tempdir_in(config)
        .map_err(|error| error.to_string())?;
    // A rename preserves the file or link without opening or following it.
    fs::rename(&state, backup.path().join(GRANTS_FILE)).map_err(|error| error.to_string())?;
    let backup = backup.keep();
    sync_directory(&backup)?;
    sync_directory(config)?;
    Ok(read(config))
}

fn verify_installed(reviewed: &InstalledManifest) -> Result<(), String> {
    let (_, current) = inspect_manifest(&reviewed.path, Ok);
    if current.as_ref() != Some(reviewed) {
        return Err(
            "The installed manifest changed. Reload extensions and review it again.".into(),
        );
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::extensions::catalog::{lua_authorization, set_lua_approved};
    use crate::app::extensions::manifest::ExtensionAction;

    const MANIFEST: &str = include_str!("../../../../examples/extensions/markdown-quote.json");
    const ID: &str = "markdown-quote";
    const COMMAND: &str = "markdown-quote.quote";

    fn candidate(path: &Path, text: &str) -> ImportPreview {
        fs::write(path, text).unwrap();
        ImportPreview::open(path.to_owned()).unwrap()
    }

    #[test]
    fn update_installs_reviewed_bytes_and_reinstall_never_recovers_prior_approval() {
        let download = tempfile::tempdir().unwrap();
        let source = download.path().join("download.json");
        let user = tempfile::tempdir().unwrap();
        let config = user.path();

        let first = candidate(&source, MANIFEST).review(config).unwrap();
        let catalog = first.install(config).unwrap();
        assert!(matches!(first.change, InstallChange::New));
        assert!(catalog.contributions.is_empty());
        let installed = catalog.view.files[0].installed.as_ref().unwrap();
        assert!(matches!(
            ImportPreview::open(source.clone())
                .unwrap()
                .review(config)
                .unwrap()
                .change,
            InstallChange::Unchanged
        ));
        let approved = set_lua_approved(
            config,
            &installed.path,
            ID,
            COMMAND,
            &first.preview.digest,
            true,
        )
        .unwrap();
        let ExtensionAction::Lua(old) = &approved.contributions[0].action else {
            panic!("missing approved command");
        };

        let evolved = MANIFEST.replace("Quote selection as Markdown", "Add Markdown quote");
        let update = candidate(&source, &evolved).review(config).unwrap();
        assert!(matches!(update.change, InstallChange::Update { .. }));
        fs::write(&source, "changed after preview").unwrap();

        let updated = update.install(config).unwrap();
        assert!(updated.contributions.is_empty());
        assert!(lua_authorization(config, old).is_none());
        assert_eq!(fs::read_to_string(&installed.path).unwrap(), evolved);

        let installed = updated.view.files[0].installed.as_ref().unwrap();
        let approved = set_lua_approved(
            config,
            &installed.path,
            ID,
            COMMAND,
            &update.preview.digest,
            true,
        )
        .unwrap();
        let ExtensionAction::Lua(updated_plugin) = &approved.contributions[0].action else {
            panic!("missing approved update");
        };
        assert!(uninstall(config, installed).unwrap().view.files.is_empty());
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            "changed after preview"
        );
        assert!(lua_authorization(config, updated_plugin).is_none());

        let reinstalled = candidate(&source, &evolved)
            .review(config)
            .unwrap()
            .install(config)
            .unwrap();
        assert!(reinstalled.contributions.is_empty());
        assert!(lua_authorization(config, updated_plugin).is_none());
    }

    #[test]
    fn stale_update_and_uninstall_never_replace_newer_manifests() {
        let download = tempfile::tempdir().unwrap();
        let source = download.path().join("download.json");
        let user = tempfile::tempdir().unwrap();
        let config = user.path();
        let catalog = candidate(&source, MANIFEST)
            .review(config)
            .unwrap()
            .install(config)
            .unwrap();
        let installed = catalog.view.files[0].installed.as_ref().unwrap();
        let update = candidate(
            &source,
            &MANIFEST.replace("Quote selection", "Format selection"),
        )
        .review(config)
        .unwrap();

        let external = MANIFEST.replace("Quote selection", "Wrap selection");
        fs::write(&installed.path, &external).unwrap();
        assert!(update.install(config).is_err());
        assert!(uninstall(config, installed).is_err());
        assert_eq!(fs::read_to_string(&installed.path).unwrap(), external);

        let duplicate = installed.path.with_file_name("duplicate.json");
        fs::write(&duplicate, &external).unwrap();
        assert!(ImportPreview::open(source).unwrap().review(config).is_err());
        let (_, duplicate) = inspect_manifest(&duplicate, Ok);
        assert!(uninstall(config, &duplicate.unwrap()).is_ok());
        assert_eq!(fs::read_to_string(&installed.path).unwrap(), external);
    }

    #[test]
    fn broken_manifest_can_be_removed_without_touching_other_files() {
        let user = tempfile::tempdir().unwrap();
        let config = user.path();
        let directory = config.join("extensions");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("broken.json");
        fs::write(&path, [255, 0, 1]).unwrap();
        let catalog = read(config);
        let broken = &catalog.view.files[0];
        assert!(broken.details.is_err());
        assert!(uninstall(config, broken.installed.as_ref().unwrap())
            .unwrap()
            .view
            .files
            .is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn resets_only_unreadable_approvals_and_preserves_the_original_file() {
        let user = tempfile::tempdir().unwrap();
        let config = user.path();
        let source = config.join("download.json");
        let review = candidate(&source, MANIFEST).review(config).unwrap();
        let catalog = review.install(config).unwrap();
        let installed = catalog.view.files[0].installed.as_ref().unwrap();
        let approved = set_lua_approved(
            config,
            &installed.path,
            ID,
            COMMAND,
            &review.preview.digest,
            true,
        )
        .unwrap();
        let ExtensionAction::Lua(old) = &approved.contributions[0].action else {
            panic!("missing approved command");
        };
        assert!(reset_unreadable_approvals(config).is_err());
        assert!(lua_authorization(config, old).is_some());

        let corrupt = b"{unreadable";
        let grants = config.join(GRANTS_FILE);
        fs::write(&grants, corrupt).unwrap();
        assert!(uninstall(config, installed).is_err());
        let reset = reset_unreadable_approvals(config).unwrap();
        assert!(!reset.view.grants_unreadable);
        assert!(reset.contributions.is_empty());
        assert!(lua_authorization(config, old).is_none());
        assert!(!grants.exists());
        let backup = fs::read_dir(config)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("lua-approvals-backup-")
            })
            .unwrap();
        assert_eq!(fs::read(backup.path().join(GRANTS_FILE)).unwrap(), corrupt);
        assert!(uninstall(config, installed).unwrap().view.files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_an_extensions_directory_link_or_replacement_manifest_link() {
        use std::os::unix::fs::symlink;

        let user = tempfile::tempdir().unwrap();
        let config = user.path();
        let external = tempfile::tempdir().unwrap();
        let source = external.path().join("example.json");
        let preview = candidate(&source, MANIFEST);
        let directory = config.join("extensions");
        symlink(external.path(), &directory).unwrap();
        assert!(preview.clone().review(config).is_err());
        assert!(read(config).view.files.is_empty());
        assert_eq!(fs::read_to_string(&source).unwrap(), MANIFEST);

        fs::remove_file(directory).unwrap();
        let installed = preview.review(config).unwrap().install(config).unwrap();
        let installed = installed.view.files[0].installed.as_ref().unwrap();
        fs::remove_file(&installed.path).unwrap();
        symlink(&source, &installed.path).unwrap();
        assert!(uninstall(config, installed).is_err());
        assert_eq!(fs::read_to_string(source).unwrap(), MANIFEST);
    }
}
