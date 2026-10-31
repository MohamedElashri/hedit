use super::*;
use std::{fs::OpenOptions, time::SystemTime};

#[derive(Clone, Copy, Debug)]
#[cfg(not(target_os = "macos"))]
enum Kind {
    Installed,
    Portable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    inode: (u64, u64),
}
fn identity(path: &Path) -> Result<Identity, String> {
    let meta = fs::symlink_metadata(path).map_err(error)?;
    if !meta.is_file() {
        return Err("The installed executable must be a regular file.".into());
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(Identity {
        length: meta.len(),
        modified: meta.modified().map_err(error)?,
        #[cfg(unix)]
        inode: (meta.dev(), meta.ino()),
    })
}
#[derive(Clone, Debug)]
pub struct Installation {
    root: PathBuf,
    executable: PathBuf,
    identity: Identity,
    #[cfg(not(target_os = "macos"))]
    kind: Kind,
}
impl Installation {
    pub fn detect() -> Result<Self, String> {
        Self::for_executable(&std::env::current_exe().map_err(error)?)
    }
    pub fn for_executable(executable: &Path) -> Result<Self, String> {
        if !matches!(std::env::consts::OS, "linux" | "macos") {
            return Err("In-app installation currently supports Linux and macOS.".into());
        }
        let executable = fs::canonicalize(executable).map_err(error)?;
        if executable.file_name().and_then(|s| s.to_str()) != Some("hedit") {
            return Err(
                "Unrecognized executable name. Use the release installer to upgrade.".into(),
            );
        }
        #[cfg(target_os = "macos")]
        let root = super::macos::installation_root(&executable)?;
        #[cfg(not(target_os = "macos"))]
        let (root, kind) = {
            let parent = executable
                .parent()
                .ok_or("Cannot locate the installation directory.")?;
            let (root, kind) = if parent.file_name().and_then(|s| s.to_str()) == Some("bin") {
                let prefix = parent.parent().ok_or("Cannot locate the install prefix.")?;
                if !prefix.join("share/doc/hedit/LICENSE").is_file()
                    || !prefix.join("share/applications/hedit.desktop").is_file()
                {
                    return Err("This installation is not recognized. Use the release installer or your package manager.".into());
                }
                (prefix.to_path_buf(), Kind::Installed)
            } else if parent.join("install.sh").is_file()
                && parent.join("README.md").is_file()
                && parent.join("themes").is_dir()
                && parent.join("assets").is_dir()
            {
                (parent.to_path_buf(), Kind::Portable)
            } else {
                return Err("Source builds and unrecognized layouts cannot update in place. Install a release package first, or rebuild from source.".into());
            };
            (root, kind)
        };
        let result = Self {
            identity: identity(&executable)?,
            executable,
            root,
            #[cfg(not(target_os = "macos"))]
            kind,
        };
        // All preparation uses this filesystem, so permissions fail before downloads.
        let probe = tempfile::Builder::new().prefix(".hedit-update-probe-").tempdir_in(&result.root)
            .map_err(|e| format!("Installation is not writable: {e}. Use the installer or package manager; hedit never asks for elevated privileges."))?;
        drop(probe);
        Ok(result)
    }
    pub fn location(&self) -> &Path {
        &self.root
    }
    pub(super) fn unchanged(&self) -> Result<(), String> {
        if identity(&self.executable)? != self.identity {
            return Err("The installation changed since this update check. Restart hedit before trying again.".into());
        }
        Ok(())
    }
    pub(super) fn lock(&self) -> Result<File, String> {
        self.unchanged()?;
        let lock_path = self.root.join(".hedit-update.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
        }
        let lock = options.open(lock_path).map_err(error)?;
        lock.try_lock()
            .map_err(|e| format!("Another hedit update is already using this installation: {e}"))?;
        Ok(lock)
    }
    pub(super) fn replace(&self, package: &Path) -> Result<(), String> {
        self.replace_with_hook(package, |_| Ok(()))
    }
    #[cfg(not(target_os = "macos"))]
    pub(super) fn replace_with_hook(
        &self,
        package: &Path,
        mut before_commit: impl FnMut(usize) -> Result<(), String>,
    ) -> Result<(), String> {
        self.unchanged()?;
        let staging = tempfile::Builder::new()
            .prefix(".hedit-update-")
            .tempdir_in(&self.root)
            .map_err(error)?;
        let new = staging.path().join("new");
        let old = staging.path().join("old");
        fs::create_dir(&new).map_err(error)?;
        fs::create_dir(&old).map_err(error)?;
        let docs = staging.path().join("docs");
        fs::create_dir(&docs).map_err(error)?;
        for name in [
            "README.md",
            "USER_GUIDE.md",
            "DEVELOPER_GUIDE.md",
            "LICENSE",
        ] {
            copy_tree(&package.join(name), &docs.join(name))?;
        }
        copy_tree(&package.join("assets"), &docs.join("assets"))?;
        let components = match self.kind {
            Kind::Installed => vec![
                (package.join("themes"), PathBuf::from("share/hedit/themes")),
                (
                    package.join("assets/hedit.svg"),
                    "share/icons/hicolor/scalable/apps/hedit.svg".into(),
                ),
                (
                    package.join("hedit.desktop"),
                    "share/applications/hedit.desktop".into(),
                ),
                (docs, "share/doc/hedit".into()),
                (package.join("hedit"), "bin/hedit".into()),
            ],
            Kind::Portable => [
                "assets",
                "themes",
                "examples",
                "README.md",
                "USER_GUIDE.md",
                "DEVELOPER_GUIDE.md",
                "LICENSE",
                "install.sh",
                "hedit.desktop",
                "hedit",
            ]
            .into_iter()
            .map(|name| (package.join(name), PathBuf::from(name)))
            .collect(),
        };
        let mut targets = Vec::new();
        // Stage everything before modifying anything. Merge resource trees so
        // additional palettes and unrelated installed documentation are retained.
        for (index, (source, relative)) in components.iter().enumerate() {
            ensure_safe_target(&self.root, relative)?;
            let target = self.root.join(relative);
            let staged = new.join(index.to_string());
            let backup = old.join(index.to_string());
            let exists = target.try_exists().map_err(error)?;
            if source.is_dir() && exists {
                copy_tree(&target, &staged)?;
            }
            copy_tree(source, &staged)?;
            let directory = fs::symlink_metadata(&staged).map_err(error)?.is_dir();
            if exists && directory != target.is_dir() {
                return Err(format!(
                    "Installation path has the wrong type: {}",
                    target.display()
                ));
            }
            if exists && !directory {
                copy_tree(&target, &backup)?;
            }
            targets.push(Target {
                path: target,
                staged,
                backup,
                existed: exists,
                directory,
                committed: false,
                moved: false,
            });
        }
        self.unchanged()?;
        let result = (|| {
            for (index, target) in targets.iter_mut().enumerate() {
                before_commit(index)?;
                if target.path == self.executable {
                    self.unchanged()?;
                }
                if target.directory && target.existed {
                    fs::rename(&target.path, &target.backup).map_err(error)?;
                    target.moved = true;
                }
                fs::rename(&target.staged, &target.path).map_err(error)?;
                target.committed = true;
                sync_parent(&target.path)?;
            }
            Ok::<_, String>(())
        })();
        if let Err(cause) = result {
            let mut failures = Vec::new();
            for target in targets.iter_mut().rev() {
                if !target.committed && !target.moved {
                    continue;
                }
                let restore = (|| {
                    if target.directory && target.committed {
                        fs::remove_dir_all(&target.path).map_err(error)?;
                    }
                    if target.existed {
                        fs::rename(&target.backup, &target.path).map_err(error)?;
                    } else if target.committed && !target.directory {
                        fs::remove_file(&target.path).map_err(error)?;
                    }
                    sync_parent(&target.path)
                })();
                if let Err(e) = restore {
                    failures.push(e);
                }
            }
            if !failures.is_empty() {
                let kept = staging.keep();
                return Err(format!(
                    "Update failed: {cause}. Recovery files were kept at {}. Rollback errors: {}",
                    kept.display(),
                    failures.join("; ")
                ));
            }
            return Err(format!(
                "Update failed; previous installation restored: {cause}"
            ));
        }
        // Binary is the last atomic rename. The running process retains its old
        // inode and stays open; session storage is never part of this transaction.
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn replace_with_hook(
        &self,
        package: &Path,
        mut before_commit: impl FnMut(usize) -> Result<(), String>,
    ) -> Result<(), String> {
        self.unchanged()?;
        let bundle = self
            .executable
            .ancestors()
            .nth(3)
            .ok_or("Missing app bundle.")?;
        let relative = bundle.strip_prefix(&self.root).map_err(error)?;
        ensure_safe_target(&self.root, relative)?;
        let staging = tempfile::Builder::new()
            .prefix(".hedit-update-")
            .tempdir_in(&self.root)
            .map_err(error)?;
        let staged = staging.path().join("hedit.app");
        // Copy the complete signed bundle without merging old resources into it.
        copy_tree(package, &staged)?;
        super::macos::verify_signature(&staged)?;
        before_commit(0)?;
        self.unchanged()?;
        let exchange = || {
            rustix::fs::renameat_with(
                rustix::fs::CWD,
                &staged,
                rustix::fs::CWD,
                bundle,
                rustix::fs::RenameFlags::EXCHANGE,
            )
            .map_err(error)
        };
        // RENAME_SWAP exchanges entire bundles atomically on the same filesystem.
        // The running executable keeps its inode; the new bundle is for next launch.
        exchange()?;
        if let Err(cause) = before_commit(1).and_then(|()| sync_parent(bundle)) {
            if let Err(rollback) = exchange().and_then(|()| sync_parent(bundle)) {
                let kept = staging.keep();
                return Err(format!(
                    "Update failed: {cause}. Recovery files were kept at {}. Rollback error: {rollback}",
                    kept.display()
                ));
            }
            return Err(format!(
                "Update failed; previous installation restored: {cause}"
            ));
        }
        Ok(())
    }
}
#[cfg(not(target_os = "macos"))]
struct Target {
    path: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    existed: bool,
    directory: bool,
    committed: bool,
    moved: bool,
}
fn ensure_safe_target(root: &Path, relative: &Path) -> Result<(), String> {
    use std::path::Component;
    let mut path = root.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        if !matches!(component, Component::Normal(_)) {
            return Err("Unsafe installation path.".into());
        }
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "Cannot update through a symlink: {}",
                    path.display()
                ))
            }
            Ok(meta) if components.peek().is_some() && !meta.is_dir() => {
                return Err(format!("Not an installation directory: {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && components.peek().is_some() => {
                fs::create_dir(&path).map_err(error)?;
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(error(e)),
            _ => (),
        }
    }
    Ok(())
}
fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(source).map_err(|e| format!("{}: {e}", source.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Update paths must not contain symlinks: {}",
            source.display()
        ));
    }
    if meta.is_dir() {
        let existed = destination.try_exists().map_err(error)?;
        fs::create_dir_all(destination).map_err(error)?;
        for entry in fs::read_dir(source).map_err(error)? {
            let entry = entry.map_err(error)?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        if !existed {
            fs::set_permissions(destination, meta.permissions()).map_err(error)?;
        }
        sync_dir(destination)?;
    } else if meta.is_file() {
        if destination.is_symlink() {
            return Err("Staged update contains a symlink.".into());
        }
        fs::copy(source, destination).map_err(error)?;
        File::open(destination)
            .map_err(error)?
            .sync_all()
            .map_err(error)?;
    } else {
        return Err("Update contains a special file.".into());
    }
    Ok(())
}
fn sync_dir(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path).map_err(error)?.sync_all().map_err(error)?;
    }
    let _ = path;
    Ok(())
}
fn sync_parent(path: &Path) -> Result<(), String> {
    sync_dir(path.parent().ok_or("Missing install parent.")?)
}
pub(super) fn verify_binary(path: &Path, version: &str) -> Result<(), String> {
    if !fs::symlink_metadata(path).map_err(error)?.is_file() {
        return Err("Release executable must be a regular file.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(error)?;
    }
    let output = tempfile::tempfile().map_err(error)?;
    run_tool(
        Command::new(path).arg("--version"),
        &output,
        4096,
        Duration::from_secs(15),
    )
    .map_err(|e| {
        format!("New executable cannot run on this system. Nothing was installed.\n{e}")
    })?;
    if read_text(&output, 4096)?.trim() != format!("hedit {version}") {
        return Err("Release executable version does not match the selected release.".into());
    }
    Ok(())
}
