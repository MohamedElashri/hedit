//! Private command storage is bound to a persistent approval token, never a path from Lua.
use super::{
    manifest::LuaPlugin,
    permissions::{open_leaf, verify_grant_owner},
};
use crate::plugin_runtime::sdk::Storage;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};
#[cfg(windows)]
mod windows;
const DIRECTORY: &str = "extensions-state";
const MAX_BYTES: u64 = 32 * 1024;
const MAX_BUCKETS: usize = 512;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u32,
    values: Storage,
}
fn regular_directory(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return false;
        }
    }
    metadata.is_dir() && !metadata.file_type().is_symlink()
}
fn directory(config: &Path, create: bool) -> Result<PathBuf, String> {
    let path = config.join(DIRECTORY);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if regular_directory(&metadata) => {}
        Ok(_) => return Err("Private storage requires a regular directory.".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if create {
                let builder = fs::DirBuilder::new();
                #[cfg(unix)]
                let builder = {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut owner_only = builder;
                    owner_only.mode(0o700);
                    owner_only
                };
                builder.create(&path).map_err(|e| e.to_string())?;
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    if path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
            if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0
            {
                return Err("Private storage requires an owner-only directory.".into());
            }
        }
        #[cfg(windows)]
        windows::protect_directory(&path).map_err(|e| e.to_string())?;
    }
    Ok(path)
}
fn owner(plugin: &LuaPlugin) -> Result<String, String> {
    if !plugin.storage {
        return Err("This command has no private storage permission.".into());
    }
    plugin
        .approval_id()
        .ok_or_else(|| "Private storage requires persistent command approval.".into())
}
fn private(file: &File) -> Result<(), String> {
    verify_grant_owner(file)?;
    #[cfg(windows)]
    windows::verify_file(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if file.metadata().map_err(|e| e.to_string())?.mode() & 0o077 != 0 {
            return Err("Private command storage must be readable only by its owner.".into());
        }
    }
    Ok(())
}
pub(in crate::app::extensions) fn read_state(
    config: &Path,
    plugin: &LuaPlugin,
) -> Result<Storage, String> {
    let path = directory(config, false)?.join(format!("{}.json", owner(plugin)?));
    let file = match open_leaf(&path, OpenOptions::new().read(true)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Storage::default()),
        Err(error) => return Err(error.to_string()),
    };
    private(&file)?;
    let bytes = hedit_core::file_io::read_bounded(file, MAX_BYTES).map_err(|e| e.to_string())?;
    let stored: Stored =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid private command storage.")?;
    if stored.version != 1 {
        return Err("Unsupported private storage version.".into());
    }
    stored.values.validate().map_err(|e| e.to_string())?;
    Ok(stored.values)
}
/// Caller retains the authorization lock through this atomic file update.
pub(in crate::app::extensions) fn write_state(
    config: &Path,
    plugin: &LuaPlugin,
    values: &Storage,
) -> Result<(), String> {
    values.validate().map_err(|e| e.to_string())?;
    let root = directory(config, true)?;
    let path = root.join(format!("{}.json", owner(plugin)?));
    if !path.exists()
        && fs::read_dir(&root)
            .map_err(|e| e.to_string())?
            .take(MAX_BUCKETS)
            .count()
            >= MAX_BUCKETS
    {
        return Err("Too many private storage buckets.".into());
    }
    match open_leaf(&path, OpenOptions::new().read(true)) {
        Ok(file) => private(&file)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    hedit::projects::atomic_json(
        &path,
        &Stored {
            version: 1,
            values: values.clone(),
        },
    )
}
/// Run after replacing grant storage, while the extension policy lock is held.
pub(super) fn prune(config: &Path, active: &BTreeSet<String>) -> Result<(), String> {
    let path = directory(config, false)?;
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    let mut files = Vec::new();
    for (count, entry) in entries.take(MAX_BUCKETS + 1).enumerate() {
        let entry = entry.map_err(|e| e.to_string())?;
        if count == MAX_BUCKETS {
            return Err("Too many private storage buckets.".into());
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            return Err("Invalid private storage filename.".into());
        };
        if name.starts_with(".tmp") {
            continue;
        }
        let token = name
            .strip_suffix(".json")
            .filter(|t| {
                t.len() == 32
                    && t.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
            .ok_or("Invalid private storage filename.")?;
        if !active.contains(token) {
            files.push(entry.path());
        }
    }
    for file in files {
        fs::remove_file(file).map_err(|e| e.to_string())?;
    }
    Ok(())
}
