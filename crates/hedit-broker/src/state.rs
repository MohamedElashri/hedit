//! Host-owned resource approvals. Call mutation methods under the editor's
//! exclusive extension-policy lock and advance its epoch before writing.
use crate::{
    declaration::Definition,
    journal::native,
    network::Endpoint,
    recovery_store::RecoveryStore,
    resources::Resources,
    workspace::{self, Identity, Policy, Protected, Right, Root},
    Handle,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
const DIRECTORY: &str = "extensions-broker";
const FILE: &str = "approvals.json";
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_APPROVALS: usize = 512;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Extension resource approvals need review.",
    )
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Review resource access for this exact command before using it.",
    )
}
fn private_child(parent: &Dir, name: &str) -> io::Result<Dir> {
    match native::private_directory(parent, name) {
        Ok(directory) => Ok(directory),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let directory = parent.open_dir_nofollow(name)?;
            hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
            Ok(directory)
        }
        Err(error) => Err(error),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedScope {
    path: PathBuf,
    identity: Identity,
    created: u64,
    rights: Vec<Right>,
}
impl SelectedScope {
    pub fn select(path: &Path, rights: Vec<Right>, state: &State) -> io::Result<Self> {
        let root = Root::open_selected(
            path,
            Policy::new(rights.iter().copied())?,
            state.protected.clone(),
        )?;
        let selected = Self {
            path: root.locator().to_owned(),
            identity: root.identity(),
            created: created(&root)?,
            rights,
        };
        selected.validate()?;
        Ok(selected)
    }
    fn validate(&self) -> io::Result<()> {
        if !self.path.is_absolute()
            || self
                .path
                .to_str()
                .is_none_or(|path| path.len() > 4096 || path.chars().any(char::is_control))
            || self.rights.len() > 4
            || self.rights.iter().collect::<BTreeSet<_>>().len() != self.rights.len()
        {
            return Err(invalid());
        }
        Policy::new(self.rights.iter().copied())?;
        Ok(())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
    pub fn rights(&self) -> &[Right] {
        &self.rights
    }
    pub fn with_rights(&self, rights: Vec<Right>) -> io::Result<Self> {
        let result = Self {
            rights,
            ..self.clone()
        };
        result.validate()?;
        Ok(result)
    }
    /// Reopen a host-selected scope without broadening its physical identity or rights.
    pub fn resolve(&self, state: &State) -> io::Result<Root> {
        self.validate()?;
        let root = Root::open_selected(
            &self.path,
            Policy::new(self.rights.iter().copied())?,
            state.protected.clone(),
        )?;
        if root.identity() != self.identity
            || root.locator() != self.path
            || created(&root)? != self.created
        {
            return Err(denied());
        }
        root.current()?;
        Ok(root)
    }
}
fn created(root: &Root) -> io::Result<u64> {
    let time = root
        .directory()
        .try_clone()?
        .into_std_file()
        .metadata()?
        .created()?;
    time.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|age| age.as_nanos().try_into().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "This filesystem cannot preserve a workspace approval identity.",
            )
        })
}
/// Bound to the existing exact-source command approval token. This record is
/// configuration data, never accepted as authority over the worker transport.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub command: String,
    pub token: Handle,
    pub scope: Option<SelectedScope>,
    pub endpoints: Vec<String>,
    pub tools: BTreeMap<String, Handle>,
}
impl Approval {
    pub fn validate(&self) -> io::Result<()> {
        if !hedit_plugin::options::valid_command_id(&self.command)
            || self.endpoints.len() > 8
            || self.tools.len() > 8
            || self.endpoints.iter().collect::<BTreeSet<_>>().len() != self.endpoints.len()
            || self
                .endpoints
                .iter()
                .chain(self.tools.keys())
                .any(|name| !hedit_plugin::options::valid_id(name))
        {
            return Err(invalid());
        }
        if let Some(scope) = &self.scope {
            scope.validate()?;
        }
        Ok(())
    }
    pub fn validate_definition(&self, definition: &Definition) -> io::Result<()> {
        self.validate()?;
        if self
            .endpoints
            .iter()
            .any(|id| !definition.endpoints.contains_key(id))
            || self
                .tools
                .keys()
                .any(|id| !definition.tools.contains_key(id))
            || self.scope.as_ref().is_some_and(|scope| {
                scope.rights.iter().any(|right| {
                    !definition
                        .capabilities
                        .iter()
                        .any(|capability| capability.workspace() == Some(*right))
                })
            })
        {
            return Err(denied());
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    approvals: Vec<Approval>,
}
pub struct Resolved {
    pub resources: Option<Resources>,
    pub endpoints: BTreeMap<String, Arc<Endpoint>>,
    pub tools: BTreeMap<String, Handle>,
}
pub struct State {
    directory: Dir,
    path: PathBuf,
    protected: Protected,
    recovery: Arc<RecoveryStore>,
}
impl State {
    /// Call off the UI thread. The whole configuration root is excluded even
    /// when the user selects an ancestor as an extension's workspace scope.
    pub fn open(
        config: &Path,
        extra_exclusions: impl IntoIterator<Item = PathBuf>,
    ) -> io::Result<Self> {
        let config = config.canonicalize()?;
        let parent = Dir::open_ambient_dir(&config, cap_std::ambient_authority())?;
        let directory = private_child(&parent, DIRECTORY)?;
        let protected = Protected::persistent(
            &directory,
            std::iter::once(config.clone()).chain(extra_exclusions),
        )?;
        let recovery = Arc::new(RecoveryStore::open(
            private_child(&directory, "recovery")?,
            &protected,
        )?);
        Ok(Self {
            directory,
            path: config.join(DIRECTORY),
            protected,
            recovery,
        })
    }
    pub fn exists(config: &Path) -> bool {
        config.join(DIRECTORY).exists()
    }
    /// Open only when reviewing/using tools, so corrupt tool storage does not
    /// disable unrelated document or workspace capabilities. Call off the UI.
    pub fn tool_store(&self) -> io::Result<Arc<crate::tool::Store>> {
        crate::tool::Store::open(
            private_child(&self.directory, "tool-images")?,
            &self.path.join("tool-images"),
            &self.protected,
        )
        .map(Arc::new)
    }
    pub fn recovery(&self) -> Arc<RecoveryStore> {
        self.recovery.clone()
    }
    pub fn protected(&self) -> Protected {
        self.protected.clone()
    }
    pub fn approvals(&self) -> io::Result<BTreeMap<String, Approval>> {
        let file = match self.directory.open_with(
            FILE,
            OpenOptions::new()
                .read(true)
                .follow(FollowSymlinks::No)
                .nonblock(true),
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(error) => return Err(error),
        };
        workspace::identity::regular(&file)?;
        let file = file.into_std();
        hedit_core::private_io::verify(&file)?;
        let bytes = hedit_core::file_io::read_bounded(file, MAX_BYTES)?;
        let document: Document = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if document.version != 1 || document.approvals.len() > MAX_APPROVALS {
            return Err(invalid());
        }
        let mut approvals = BTreeMap::new();
        for approval in document.approvals {
            approval.validate()?;
            if approvals
                .insert(approval.command.clone(), approval)
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(approvals)
    }
    /// Caller holds the shared policy guard for the full resolution/delivery.
    pub fn resolve(
        &self,
        command: &str,
        token: Handle,
        definition: &Definition,
    ) -> io::Result<Resolved> {
        let mut approvals = self.approvals()?;
        let approval = approvals
            .remove(command)
            .filter(|approval| approval.token == token)
            .ok_or_else(denied)?;
        approval.validate_definition(definition)?;
        let resources = approval
            .scope
            .as_ref()
            .map(|scope| scope.resolve(self).and_then(Resources::new))
            .transpose()?;
        let endpoints = approval
            .endpoints
            .iter()
            .map(|id| {
                Ok((
                    id.clone(),
                    Arc::new(definition.endpoints[id].endpoint().map_err(|_| invalid())?),
                ))
            })
            .collect::<io::Result<_>>()?;
        Ok(Resolved {
            resources,
            endpoints,
            tools: approval.tools,
        })
    }
    fn write(&self, approvals: BTreeMap<String, Approval>) -> io::Result<()> {
        if approvals.len() > MAX_APPROVALS {
            return Err(invalid());
        }
        for approval in approvals.values() {
            approval.validate()?;
        }
        let bytes = serde_json::to_vec(&Document {
            version: 1,
            approvals: approvals.into_values().collect(),
        })
        .map_err(|_| invalid())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        let name = format!(".approvals-{}", Handle::fresh()?);
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = self.directory.open_with(&name, &options)?;
            hedit_core::private_io::verify(&file.try_clone()?.into_std())?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            self.directory.rename(&name, &self.directory, FILE)?;
            native::sync_dir(&self.directory)
        })();
        if result.is_err() {
            let _ = self.directory.remove_file(&name);
        }
        result
    }
    pub fn approve(&self, approval: Approval, definition: &Definition) -> io::Result<()> {
        approval.validate_definition(definition)?;
        let mut approvals = self.approvals()?;
        approvals.insert(approval.command.clone(), approval);
        self.write(approvals)
    }
    pub fn revoke(&self, command: &str) -> io::Result<()> {
        let mut approvals = self.approvals()?;
        approvals.remove(command);
        self.write(approvals)
    }
    pub fn prune(&self, tokens: &BTreeSet<Handle>) -> io::Result<()> {
        let mut approvals = self.approvals()?;
        let before = approvals.len();
        approvals.retain(|_, approval| tokens.contains(&approval.token));
        if approvals.len() != before {
            self.write(approvals)?;
        }
        Ok(())
    }
    pub fn reset(&self) -> io::Result<()> {
        match self.directory.symlink_metadata(FILE) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                let backup = format!("approvals-backup-{}.json", Handle::fresh()?);
                self.directory.rename(FILE, &self.directory, backup)?;
                native::sync_dir(&self.directory)?;
            }
            Ok(_) => return Err(invalid()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.write(BTreeMap::new())
    }
}
#[cfg(test)]
mod tests;
