//! Command-scoped consent records and the lock that serializes extension changes.
use super::{manifest, FileIdentity, InstalledManifest};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io,
    path::Path,
    time::{Duration, Instant},
};

pub(super) const DISABLED_FILE: &str = "extensions-disabled.json";
pub(super) const MAX_DISABLED: usize = 256;
const MAX_DISABLED_BYTES: u64 = 16 * 1024;
pub(super) const GRANTS_FILE: &str = "extensions-lua-grants.json";
const GRANTS_VERSION: u32 = 1;
const MAX_GRANTS: usize = manifest::MAX_MANIFESTS * manifest::MAX_COMMANDS;
const MAX_GRANTS_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app::extensions) struct Grant {
    pub(super) command: String,
    digest: String,
    manifest: String,
    identity: FileIdentity,
    capabilities: [manifest::Permission; 2],
    token: [u8; 16],
}

impl Grant {
    pub(super) fn new(command: String, installed: &InstalledManifest) -> Result<Self, String> {
        let manifest = installed
            .path
            .file_name()
            .and_then(OsStr::to_str)
            .filter(|name| valid_filename(name))
            .ok_or("The installed Lua manifest needs a UTF-8 JSON filename.")?;
        let mut token = [0; 16];
        getrandom::fill(&mut token).map_err(|_| "Could not generate a Lua approval token.")?;
        Ok(Self {
            command,
            digest: installed.digest.clone(),
            manifest: manifest.to_owned(),
            identity: installed.identity.clone(),
            capabilities: manifest::LUA_CAPABILITIES,
            token,
        })
    }

    pub(super) fn matches(&self, installed: &InstalledManifest) -> bool {
        installed.path.file_name() == Some(OsStr::new(&self.manifest))
            && self.digest == installed.digest
            && self.identity == installed.identity
            && self.capabilities == manifest::LUA_CAPABILITIES
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GrantDocument {
    version: u32,
    grants: Vec<Grant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyGrant {
    id: String,
    digest: String,
    // Decode the exact retired format without ever retaining its authority.
    #[serde(rename = "token")]
    _token: [u8; 16],
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredGrants {
    Versioned(GrantDocument),
    Legacy(Vec<LegacyGrant>),
}

#[derive(Default)]
pub(super) struct Grants {
    pub(super) commands: BTreeMap<String, Grant>,
    legacy: bool,
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_filename(name: &str) -> bool {
    let path = Path::new(name);
    name.len() <= 255
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
        && path.file_name() == Some(OsStr::new(name))
        && path.extension() == Some(OsStr::new("json"))
}

impl Grants {
    pub(super) fn read(config: &Path) -> Result<Self, String> {
        let Some(stored) = read_optional(&config.join(GRANTS_FILE), MAX_GRANTS_BYTES)? else {
            return Ok(Self::default());
        };
        match stored {
            StoredGrants::Versioned(document) => {
                if document.version != GRANTS_VERSION {
                    return Err("Unsupported Lua approval file version.".into());
                }
                if document.grants.len() > MAX_GRANTS {
                    return Err("Too many Lua command approvals.".into());
                }
                let mut commands = BTreeMap::new();
                for grant in document.grants {
                    if !super::super::valid_command_id(&grant.command)
                        || !valid_digest(&grant.digest)
                        || !valid_filename(&grant.manifest)
                        || grant.capabilities != manifest::LUA_CAPABILITIES
                        || commands.insert(grant.command.clone(), grant).is_some()
                    {
                        return Err("Invalid Lua command approvals.".into());
                    }
                }
                Ok(Self {
                    commands,
                    legacy: false,
                })
            }
            StoredGrants::Legacy(records) => {
                let mut ids = BTreeSet::new();
                if records.len() > manifest::MAX_MANIFESTS * 2
                    || records.into_iter().any(|record| {
                        !manifest::valid_id(&record.id)
                            || !valid_digest(&record.digest)
                            || !ids.insert(record.id)
                    })
                {
                    return Err("Invalid retired Lua approvals.".into());
                }
                Ok(Self {
                    commands: BTreeMap::new(),
                    legacy: true,
                })
            }
        }
    }

    pub(super) fn write(self, config: &Path) -> Result<(), String> {
        let document = GrantDocument {
            version: GRANTS_VERSION,
            grants: self.commands.into_values().collect(),
        };
        hedit::projects::atomic_json(&config.join(GRANTS_FILE), &document)?;
        #[cfg(target_os = "linux")]
        File::open(config)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("Could not sync Lua approvals: {error}"))?;
        Ok(())
    }
}

/// Call with the extension change lock held, before updating or removing the manifest.
pub(in crate::app::extensions) fn invalidate_lua_grants(
    config: &Path,
    id: Option<&str>,
    path: &Path,
) -> Result<(), String> {
    let mut grants = Grants::read(config)?;
    let before = grants.commands.len();
    grants.commands.retain(|command, grant| {
        let owner = command.split_once('.').map(|(plugin, _)| plugin);
        (id.is_none() || id != owner) && path.file_name() != Some(OsStr::new(&grant.manifest))
    });
    if grants.legacy || before != grants.commands.len() {
        grants.write(config)?;
    }
    Ok(())
}

pub(super) fn disabled(config: &Path) -> Result<BTreeSet<String>, String> {
    let ids: Vec<String> =
        read_optional(&config.join(DISABLED_FILE), MAX_DISABLED_BYTES)?.unwrap_or_default();
    if ids.len() > MAX_DISABLED || ids.iter().any(|id| !manifest::valid_id(id)) {
        return Err("Invalid disabled extension IDs.".into());
    }
    let unique: BTreeSet<_> = ids.iter().cloned().collect();
    if unique.len() != ids.len() {
        return Err("Duplicate disabled extension IDs.".into());
    }
    Ok(unique)
}

fn read_optional<T: serde::de::DeserializeOwned>(
    path: &Path,
    limit: u64,
) -> Result<Option<T>, String> {
    match open_leaf(path, OpenOptions::new().read(true)) {
        Ok(file) => {
            if path.file_name() == Some(OsStr::new(GRANTS_FILE)) {
                verify_grant_owner(&file)?;
            }
            let bytes = hedit_core::file_io::read_bounded(file, limit)
                .map_err(|error| error.to_string())?;
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn verify_grant_owner(file: &File) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0 {
            return Err(
                "Lua approval storage must be owned by you and writable only by you.".into(),
            );
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = file;
    Ok(())
}

pub(super) fn change_lock(config: &Path) -> Result<File, String> {
    fs::create_dir_all(config).map_err(|error| error.to_string())?;
    let file = open_leaf(
        &config.join("extensions.lock"),
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true),
    )
    .map_err(|error| format!("Could not open the extension change lock: {error}"))?;
    // A concurrent fork briefly inherits other threads' CLOEXEC descriptors.
    // Closing our copy can therefore leave a lock held until the child execs.
    // These operations run on workers; allow that transient lease to close,
    // while keeping a genuinely busy extension change bounded and exclusive.
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock)
                if started.elapsed() < Duration::from_millis(250) =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                return Err(format!(
                    "Could not lock extension changes: {error}. Try again."
                ));
            }
        }
    }
}

fn open_leaf(path: &Path, options: &mut OpenOptions) -> io::Result<File> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::OFlags;
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32);
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.nlink() != 1 {
            return Err(io::Error::other(
                "Extension control files must have a single link.",
            ));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other(
                "Choose a regular file without a reparse point.",
            ));
        }
    }
    if !metadata.is_file() {
        return Err(io::Error::other("Choose a regular file."));
    }
    Ok(file)
}
