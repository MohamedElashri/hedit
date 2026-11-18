//! Managed files are private to hedit. External executables are never removed.
use super::error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const DOWNLOAD_LIMIT: u64 = 256 * 1024 * 1024;
const EXPANDED_LIMIT: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub languages: Vec<String>,
    pub repository: String,
    pub requirements: String,
}
pub fn catalog() -> Vec<CatalogEntry> {
    let entries = [
        (
            "rust-analyzer",
            "Rust",
            "rust",
            "rust-lang/rust-analyzer",
            "Rust toolchain and project dependencies",
        ),
        (
            "clangd",
            "C / C++",
            "c,cpp,cuda",
            "clangd/clangd",
            "C/C++ toolchain; compile_commands.json recommended",
        ),
        (
            "ty",
            "Python · ty",
            "python",
            "astral-sh/ty",
            "Python environment and project dependencies",
        ),
        (
            "ruff",
            "Python · Ruff",
            "python",
            "astral-sh/ruff",
            "Python linting and formatting; works alongside ty",
        ),
        (
            "lua-language-server",
            "Lua",
            "lua",
            "LuaLS/lua-language-server",
            "Lua project; bundled runtime included",
        ),
        (
            "tombi",
            "TOML · Tombi",
            "toml",
            "tombi-toml/tombi",
            "TOML completion, validation and formatting",
        ),
        (
            "gopls",
            "Go",
            "go",
            "",
            "Install gopls with your Go toolchain, then detect its executable",
        ),
        (
            "typescript-language-server",
            "JavaScript / TypeScript",
            "javascript,typescript",
            "",
            "Install typescript and typescript-language-server with npm",
        ),
        (
            "vscode-html-language-server",
            "HTML",
            "html",
            "",
            "Install vscode-langservers-extracted with npm",
        ),
        (
            "vscode-css-language-server",
            "CSS",
            "css",
            "",
            "Install vscode-langservers-extracted with npm",
        ),
        (
            "vscode-json-language-server",
            "JSON",
            "json",
            "",
            "Install vscode-langservers-extracted with npm",
        ),
        (
            "yaml-language-server",
            "YAML",
            "yaml",
            "",
            "Install yaml-language-server with npm",
        ),
        (
            "bash-language-server",
            "Shell",
            "shell",
            "",
            "Install bash-language-server with npm",
        ),
        (
            "taplo",
            "TOML · Taplo",
            "toml",
            "",
            "Install a Taplo build with language-server support",
        ),
        (
            "pylsp",
            "Python · pylsp",
            "python",
            "",
            "Install python-lsp-server in your Python environment",
        ),
    ];
    entries
        .into_iter()
        .map(
            |(id, name, languages, repository, requirements)| CatalogEntry {
                id: id.into(),
                name: name.into(),
                languages: languages.split(',').map(str::to_owned).collect(),
                repository: repository.into(),
                requirements: requirements.into(),
            },
        )
        .collect()
}
pub fn default_arguments(id: &str) -> Vec<String> {
    match id {
        "ty" | "ruff" => vec!["server".into()],
        "tombi" => vec!["lsp".into()],
        "taplo" => vec!["lsp".into(), "stdio".into()],
        "bash-language-server" => vec!["start".into()],
        "typescript-language-server"
        | "vscode-html-language-server"
        | "vscode-css-language-server"
        | "vscode-json-language-server"
        | "yaml-language-server" => vec!["--stdio".into()],
        _ => vec![],
    }
}
/// Defaults for a new registration; existing registrations retain their settings.
pub fn preset(id: &str) -> Registration {
    let mut registration = Registration {
        languages: catalog()
            .into_iter()
            .find(|e| e.id == id)
            .map(|e| e.languages)
            .unwrap_or_default(),
        initialization_options: Some(serde_json::json!({})),
        ..Registration::default()
    };
    registration.arguments = catalog()
        .iter()
        .find(|e| e.id == id)
        .filter(|e| e.repository.is_empty())
        .map(|_| default_arguments(id));
    let section = match id {
        "lua-language-server" => Some("Lua"),
        "vscode-html-language-server" => Some("html"),
        "vscode-css-language-server" => Some("css"),
        "vscode-json-language-server" => Some("json"),
        "yaml-language-server" => Some("yaml"),
        "bash-language-server" => Some("bashIde"),
        _ => None,
    };
    if let Some(section) = section {
        registration.configuration = serde_json::json!({(section):{}});
    }
    if id == "typescript-language-server" {
        registration.configuration = serde_json::json!({"typescript":{},"javascript":{}});
    }
    if id == "ty" {
        registration.priority = 50;
        registration.features.formatting = false;
    }
    if id == "ruff" {
        registration.features = Features {
            completion: false,
            hover: false,
            definition: false,
            references: false,
            ..Features::default()
        };
        registration.initialization_options = Some(serde_json::json!({"settings":{}}));
    }
    registration
}
/// Find a preset executable on this host. Never searches relative PATH entries.
/// Detection does not launch the program or change a registration.
pub fn detect(id: &str) -> Result<PathBuf, String> {
    if !catalog().iter().any(|e| e.id == id) {
        return Err("Detection requires a known server preset; enter an absolute executable path for custom servers.".into());
    }
    let path = std::env::var_os("PATH").ok_or("PATH is not available on this host.")?;
    detect_in(id, &path)
}
fn detect_in(id: &str, path: &std::ffi::OsStr) -> Result<PathBuf, String> {
    let mut names = vec![id.to_owned()];
    if cfg!(windows) {
        // Shell wrappers (.cmd/.bat) are not direct stdio executables. A native
        // .exe or an explicit runtime launch is required for predictable argv.
        names = vec![format!("{id}.exe")];
    }
    for directory in std::env::split_paths(path)
        .take(512)
        .filter(|p| p.is_absolute())
    {
        for name in &names {
            let candidate = directory.join(name);
            if !candidate.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if !fs::metadata(&candidate).is_ok_and(|m| m.permissions().mode() & 0o111 != 0) {
                    continue;
                }
            }
            return Ok(candidate);
        }
    }
    Err(format!("{id} was not found on this host's PATH. Install it separately or choose an absolute executable path."))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    pub completion: bool,
    pub hover: bool,
    pub definition: bool,
    pub references: bool,
    pub formatting: bool,
    pub rename: bool,
    pub quick_fixes: bool,
    pub diagnostics: bool,
}
impl Default for Features {
    fn default() -> Self {
        Self {
            completion: true,
            hover: true,
            definition: true,
            references: true,
            formatting: true,
            rename: true,
            quick_fixes: true,
            diagnostics: true,
        }
    }
}
impl Features {
    pub fn any_enabled(&self) -> bool {
        self.completion
            || self.hover
            || self.definition
            || self.references
            || self.rename
            || self.quick_fixes
            || self.formatting
            || self.diagnostics
    }
    pub fn enabled(&self, feature: super::Feature) -> bool {
        match feature {
            super::Feature::Completion => self.completion,
            super::Feature::Hover => self.hover,
            super::Feature::Definition => self.definition,
            super::Feature::References => self.references,
            super::Feature::Formatting => self.formatting,
            super::Feature::Rename => self.rename,
            super::Feature::QuickFixes => self.quick_fixes,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub version: String,
    pub digest: String,
    pub source: String,
    pub directory: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Registration {
    pub languages: Vec<String>,
    pub enabled: bool,
    pub external: Option<Launch>,
    pub managed: Option<Receipt>,
    pub configuration: serde_json::Value,
    /// None retains the pre-0.5.2 behavior of using configuration at initialize.
    pub initialization_options: Option<serde_json::Value>,
    pub arguments: Option<Vec<String>>,
    pub environment: BTreeMap<String, String>,
    pub root_markers: Vec<String>,
    pub priority: i16,
    pub features: Features,
    pub workspaces: BTreeMap<String, bool>,
}
impl Default for Registration {
    fn default() -> Self {
        Self {
            languages: vec![],
            enabled: true,
            external: None,
            managed: None,
            configuration: serde_json::json!({}),
            workspaces: BTreeMap::new(),
            initialization_options: None,
            arguments: None,
            environment: BTreeMap::new(),
            root_markers: vec![],
            priority: 0,
            features: Features::default(),
        }
    }
}
impl Registration {
    pub fn enabled_at(&self, root: &Path) -> bool {
        self.workspaces
            .get(&root.to_string_lossy().into_owned())
            .copied()
            .unwrap_or(self.enabled)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installed {
    pub id: String,
    pub registration: Registration,
}
#[derive(Clone)]
pub struct Manager {
    root: PathBuf,
}
impl Manager {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn validate_id(id: &str) -> Result<(), String> {
        if id.is_empty()
            || id.len() > 80
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(
                "Server ID must contain 1–80 letters, digits, underscores or hyphens.".into(),
            );
        }
        Ok(())
    }
    fn directory(&self, id: &str) -> Result<PathBuf, String> {
        Self::validate_id(id)?;
        Ok(self.root.join(id))
    }
    fn prepare(&self, id: &str) -> Result<PathBuf, String> {
        let dir = self.directory(id)?;
        let new_root = !self.root.exists();
        fs::create_dir_all(&self.root).map_err(error)?;
        let metadata = fs::symlink_metadata(&self.root).map_err(error)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Language-server storage must be a real directory.".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if new_root {
                fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
                    .map_err(error)?;
            }
            let metadata = fs::symlink_metadata(&self.root).map_err(error)?;
            if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0
            {
                return Err(
                    "Language-server storage must be owned by you and not writable by other users."
                        .into(),
                );
            }
        }
        let _ = new_root;
        fs::create_dir_all(&dir).map_err(error)?;
        // Never operate through a substituted server directory.
        if fs::symlink_metadata(&dir)
            .map_err(error)?
            .file_type()
            .is_symlink()
        {
            return Err("Language-server directory is a symbolic link.".into());
        }
        Ok(dir)
    }
    fn lock(&self, id: &str, shared: bool) -> Result<File, String> {
        let dir = self.prepare(id)?;
        let lock_path = dir.join("lock");
        if fs::symlink_metadata(&lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Language-server lock is a symbolic link.".into());
        }
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            );
        }
        let file = options.open(lock_path).map_err(error)?;
        if !file.metadata().map_err(error)?.is_file() {
            return Err("Language-server lock must be a regular file.".into());
        }
        // Concurrent fork/exec briefly inherits other threads' CLOEXEC
        // descriptors. Allow that transient lease to close before reporting a
        // genuinely busy installation; all acquisition happens on workers.
        let started = Instant::now();
        loop {
            match if shared{file.try_lock_shared()}else{file.try_lock()}{
                Ok(())=>return Ok(file),
                Err(_) if started.elapsed()<Duration::from_millis(250)=>std::thread::sleep(Duration::from_millis(5)),
                Err(_)=>return Err("This server is in use or being changed by another hedit window. Stop it there and retry.".into()),
            }
        }
    }
    pub fn list(&self) -> Result<Vec<Installed>, String> {
        if !self.root.exists() {
            return Ok(vec![]);
        }
        let mut entries = vec![];
        for entry in fs::read_dir(&self.root).map_err(error)? {
            let entry = entry.map_err(error)?;
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if Self::validate_id(&id).is_err() || !entry.file_type().map_err(error)?.is_dir() {
                continue;
            }
            if let Some(registration) = self.read(&id)? {
                entries.push(Installed { id, registration });
            }
            if entries.len() > 128 {
                return Err("Too many registered language servers.".into());
            }
        }
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(entries)
    }
    pub fn read(&self, id: &str) -> Result<Option<Registration>, String> {
        let path = self.directory(id)?.join("server.json");
        match crate::file_io::open_regular(&path) {
            Ok(file) => {
                let data = crate::file_io::read_bounded(file, 1024 * 1024).map_err(error)?;
                let registration: Registration = serde_json::from_slice(&data)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                validate(&registration)?;
                Ok(Some(registration))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(error(e)),
        }
    }
    pub fn configure(&self, id: &str, registration: Registration) -> Result<(), String> {
        validate(&registration)?;
        let _lock = self.lock(id, false)?;
        let mut registration = registration;
        // Receipts can only be created by a verified managed installation.
        registration.managed = self.read(id)?.and_then(|r| r.managed);
        self.write(id, &registration)
    }
    fn write(&self, id: &str, registration: &Registration) -> Result<(), String> {
        validate(registration)?;
        let dir = self.prepare(id)?;
        let mut temp = tempfile::NamedTempFile::new_in(&dir).map_err(error)?;
        serde_json::to_writer_pretty(&mut temp, registration).map_err(error)?;
        temp.flush().map_err(error)?;
        temp.as_file().sync_all().map_err(error)?;
        temp.persist(dir.join("server.json")).map_err(error)?;
        sync_dir(&dir)
    }
    pub fn launch(&self, id: &str) -> Result<(Launch, File), String> {
        let lease = self.lock(id, true)?;
        let registration = self
            .read(id)?
            .ok_or("Server is not installed or registered.")?;
        let mut launch = if let Some(external) = registration.external {
            external
        } else if let Some(r) = registration.managed {
            let base = self.directory(id)?.join(&r.directory);
            if fs::symlink_metadata(&base)
                .map_err(error)?
                .file_type()
                .is_symlink()
            {
                return Err("Managed installation is a symbolic link.".into());
            }
            Launch {
                executable: base.join(r.executable),
                arguments: r.arguments,
            }
        } else {
            return Err(
                "Server is not installed. Select Install or configure an executable.".into(),
            );
        };
        if let Some(arguments) = registration.arguments {
            launch.arguments = arguments;
        }
        if !launch.executable.is_absolute() {
            return Err("Choose an absolute executable path.".into());
        }
        let metadata = fs::metadata(&launch.executable)
            .map_err(|e| format!("{}: {e}", launch.executable.display()))?;
        if !metadata.is_file() {
            return Err("Language server must be a regular executable file.".into());
        }
        Ok((launch, lease))
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let _lock = self.lock(id, false)?;
        let dir = self.directory(id)?;
        // Unregister atomically first. Cleanup only installation directories
        // created by hedit; never follow receipt paths to external files.
        let path = dir.join("server.json");
        if path.exists() {
            fs::remove_file(path).map_err(error)?;
            sync_dir(&dir)?;
        }
        for entry in fs::read_dir(&dir).map_err(error)? {
            let entry = entry.map_err(error)?;
            if entry.file_name().to_string_lossy().starts_with("install-")
                && entry.file_type().map_err(error)?.is_dir()
            {
                fs::remove_dir_all(entry.path()).map_err(error)?;
            }
        }
        Ok(())
    }
    pub fn install(
        &self,
        id: &str,
        cancel: &AtomicBool,
        progress: impl Fn(&str),
    ) -> Result<(), String> {
        let entry = catalog()
            .into_iter()
            .find(|e| e.id == id && !e.repository.is_empty())
            .ok_or("This server has no managed installer; configure an existing executable.")?;
        let _lock = self.lock(id, false)?;
        let dir = self.prepare(id)?;
        for entry in fs::read_dir(&dir).map_err(error)?.flatten() {
            if entry.file_name().to_string_lossy().starts_with(".staging-")
                && entry.file_type().is_ok_and(|t| t.is_dir())
            {
                fs::remove_dir_all(entry.path()).map_err(error)?;
            }
        }
        let stage = tempfile::Builder::new()
            .prefix(".staging-")
            .tempdir_in(&dir)
            .map_err(error)?;
        progress("Resolving release…");
        let metadata = stage.path().join("release.json");
        download(
            &format!(
                "https://api.github.com/repos/{}/releases/latest",
                entry.repository
            ),
            &metadata,
            2 * 1024 * 1024,
            cancel,
        )?;
        let value: serde_json::Value = serde_json::from_slice(
            &crate::file_io::read_bounded(File::open(metadata).map_err(error)?, 2 * 1024 * 1024)
                .map_err(error)?,
        )
        .map_err(error)?;
        let package = Package::resolve(
            id,
            &entry.repository,
            &value,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )?;
        progress("Downloading and verifying…");
        let archive = stage.path().join("package");
        download(&package.url, &archive, DOWNLOAD_LIMIT, cancel)?;
        self.activate(id, &package, &archive, cancel, progress)
    }
    fn activate(
        &self,
        id: &str,
        package: &Package,
        archive: &Path,
        cancel: &AtomicBool,
        progress: impl Fn(&str),
    ) -> Result<(), String> {
        check_cancel(cancel)?;
        let mut file = File::open(archive).map_err(error)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut size = 0;
        loop {
            check_cancel(cancel)?;
            let n = file.read(&mut buffer).map_err(error)?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > DOWNLOAD_LIMIT {
                return Err("Server archive is too large.".into());
            }
            hasher.update(&buffer[..n]);
        }
        if size != package.size
            || hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                != package.digest
        {
            return Err(
                "Language-server archive checksum or size does not match its release.".into(),
            );
        }
        progress("Installing…");
        let dir = self.prepare(id)?;
        let stage = tempfile::Builder::new()
            .prefix("install-")
            .tempdir_in(&dir)
            .map_err(error)?;
        unpack(archive, stage.path(), package, cancel)?;
        let executable = stage.path().join(&package.executable);
        if !executable.is_file() {
            return Err("The server archive does not contain its executable.".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).map_err(error)?;
        }
        check_cancel(cancel)?;
        if package.probe {
            progress("Checking executable…");
            probe_executable(&executable, cancel, Duration::from_secs(10))?;
        }
        let mut registration = self.read(id)?.unwrap_or_else(|| preset(id));
        if registration.languages.is_empty() {
            registration.languages = catalog()
                .into_iter()
                .find(|e| e.id == id)
                .unwrap()
                .languages;
        }
        registration.external = None;
        let receipt = Receipt {
            version: package.version.clone(),
            digest: package.digest.clone(),
            source: package.url.clone(),
            directory: stage
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            executable: package.executable.clone(),
            arguments: default_arguments(id),
        };
        let previous = registration.managed.as_ref().map(|r| r.directory.clone());
        let current = receipt.directory.clone();
        registration.managed = Some(receipt);
        sync_tree_dirs(stage.path(), cancel)?;
        // Keep the immutable directory before publishing its receipt. A crash
        // can leave an unused installation, never a receipt to deleted files.
        progress("Activating…");
        check_cancel(cancel)?;
        let _installed = stage.keep();
        // A write error can occur after the receipt was atomically replaced
        // (for example during directory sync). Preserve the candidate in that
        // uncertain case so a published receipt never points to deleted files.
        self.write(id,&registration).map_err(|e|format!("Cannot confirm server activation: {e}. Installation files were preserved; refresh to inspect the active version."))?;
        for entry in fs::read_dir(&dir).map_err(error)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("install-")
                && name != current
                && previous.as_ref() != Some(&name)
                && entry.file_type().is_ok_and(|t| t.is_dir())
            {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
        Ok(())
    }
}
pub fn validate(registration: &Registration) -> Result<(), String> {
    if serde_json::to_vec_pretty(registration)
        .map_err(error)?
        .len()
        > 1024 * 1024
    {
        return Err("Server configuration is too large.".into());
    }

    if registration.languages.is_empty()
        || registration.languages.len() > 32
        || registration.languages.iter().any(|s| {
            s.is_empty()
                || s.len() > 80
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+'))
        })
    {
        return Err("Specify 1–32 language IDs, such as rust, python, or cpp.".into());
    }
    if !registration.configuration.is_object()
        || serde_json::to_vec(&registration.configuration)
            .map_err(error)?
            .len()
            > 256 * 1024
        || registration.workspaces.len() > 1024
    {
        return Err("Server configuration must be a bounded JSON object.".into());
    }
    let valid_arguments = |args: &[String]| {
        args.len() <= 128 && args.iter().all(|s| s.len() <= 8192 && !s.contains('\0'))
    };
    if registration
        .arguments
        .as_ref()
        .is_some_and(|a| !valid_arguments(a))
        || registration.environment.len() > 128
        || registration.environment.iter().any(|(k, v)| {
            k.is_empty()
                || k.len() > 256
                || k.contains(['=', '\0'])
                || v.len() > 8192
                || v.contains('\0')
        })
        || registration.root_markers.len() > 32
        || registration
            .root_markers
            .iter()
            .any(|s| s.len() > 256 || s.contains('\0') || !safe_relative(Path::new(s)))
        || registration
            .initialization_options
            .as_ref()
            .is_some_and(|v| {
                !v.is_object() || serde_json::to_vec(v).map_or(true, |b| b.len() > 256 * 1024)
            })
    {
        return Err(
            "Invalid arguments, environment, root markers or initialization options.".into(),
        );
    }
    if let Some(launch) = &registration.external {
        if !launch.executable.is_absolute()
            || launch.executable.as_os_str().len() > 4096
            || launch.executable.to_string_lossy().contains('\0')
            || launch.arguments.len() > 128
            || launch
                .arguments
                .iter()
                .any(|s| s.len() > 8192 || s.contains('\0'))
        {
            return Err("Choose an absolute executable path and valid arguments.".into());
        }
    }
    if let Some(r) = &registration.managed {
        if !r.directory.starts_with("install-")
            || Path::new(&r.directory).components().count() != 1
            || !safe_relative(&r.executable)
            || !valid_arguments(&r.arguments)
        {
            return Err("Invalid managed installation receipt.".into());
        }
    }
    Ok(())
}
fn sync_tree_dirs(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut directories = vec![root.to_owned()];
        let mut at = 0;
        while at < directories.len() {
            check_cancel(cancel)?;
            for entry in fs::read_dir(&directories[at]).map_err(error)? {
                let entry = entry.map_err(error)?;
                if entry.file_type().map_err(error)?.is_dir() {
                    directories.push(entry.path());
                }
            }
            at += 1;
        }
        for directory in directories.iter().rev() {
            check_cancel(cancel)?;
            sync_dir(directory)?;
        }
    }
    let _ = (root, cancel);
    Ok(())
}
fn sync_dir(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path).and_then(|f| f.sync_all()).map_err(error)?;
    }
    let _ = path;
    Ok(())
}
pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Language-server operation cancelled.".into())
    } else {
        Ok(())
    }
}
struct ChildProcess(std::process::Child);
impl Drop for ChildProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn probe_executable(
    executable: &Path,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<(), String> {
    check_cancel(cancel)?;
    let stdout = tempfile::tempfile().map_err(error)?;
    let mut stderr = tempfile::tempfile().map_err(error)?;
    let mut command = Command::new(executable);
    command
        .arg("--version")
        .current_dir(
            executable
                .parent()
                .ok_or("Invalid server executable path.")?,
        )
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(error)?)
        .stderr(stderr.try_clone().map_err(error)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = ChildProcess(
        command
            .spawn()
            .map_err(|e| format!("The downloaded server cannot run on this host: {e}"))?,
    );
    let start = Instant::now();
    loop {
        check_cancel(cancel)?;
        let out = stdout.metadata().map_err(error)?.len();
        let err = stderr.metadata().map_err(error)?.len();
        if out > 65536 || err > 65536 || start.elapsed() > timeout {
            return Err("Server version check timed out or exceeded its output limit; the active installation was preserved.".into());
        }
        if let Some(status) = child.0.try_wait().map_err(error)? {
            // The child can finish between the earlier size reads and wait.
            let out = stdout.metadata().map_err(error)?.len();
            let err = stderr.metadata().map_err(error)?.len();
            if out > 65536 || err > 65536 {
                return Err("Server version output exceeded its limit.".into());
            }
            if status.success() && out + err > 0 {
                return Ok(());
            }
            stderr.seek(SeekFrom::Start(0)).map_err(error)?;
            let mut detail = String::new();
            stderr
                .take(4096)
                .read_to_string(&mut detail)
                .map_err(error)?;
            return Err(format!("The downloaded server failed its version check ({status}). Check its system requirements. {}",detail.trim()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn download(url: &str, path: &Path, limit: u64, cancel: &AtomicBool) -> Result<(), String> {
    check_cancel(cancel)?;
    let stderr = tempfile::tempfile().map_err(error)?;
    let mut command = Command::new("curl");
    command
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "180",
            "--max-redirs",
            "5",
            "--max-filesize",
            &limit.to_string(),
            "--user-agent",
            "hedit-language-servers",
            "--output",
        ])
        .arg(path)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr.try_clone().map_err(error)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = ChildProcess(
        command
            .spawn()
            .map_err(|e| format!("Cannot run curl: {e}"))?,
    );
    let start = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed)
            || start.elapsed() > Duration::from_secs(185)
            || fs::metadata(path).is_ok_and(|m| m.len() > limit)
        {
            let _ = child.0.kill();
            let _ = child.0.wait();
            return Err("Download cancelled, timed out, or exceeded its size limit.".into());
        }
        if let Some(status) = child.0.try_wait().map_err(error)? {
            if status.success() {
                return Ok(());
            }
            return Err(format!(
                "Language-server download failed ({status}). Check network access and curl."
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[derive(Debug)]
struct Package {
    version: String,
    url: String,
    digest: String,
    size: u64,
    executable: PathBuf,
    gzip: bool,
    tar: bool,
    probe: bool,
}
impl Package {
    fn resolve(
        id: &str,
        repository: &str,
        value: &serde_json::Value,
        os: &str,
        arch: &str,
    ) -> Result<Self, String> {
        if value["draft"].as_bool() != Some(false) || value["prerelease"].as_bool() != Some(false) {
            return Err("Only stable published server releases can be installed.".into());
        }
        let version = value["tag_name"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() < 100)
            .ok_or("Missing release version.")?
            .to_owned();
        let (name, executable) = match (id, os, arch) {
            ("rust-analyzer", os, "x86_64" | "aarch64") => {
                let target = match os {
                    "linux" => "unknown-linux-gnu",
                    "macos" => "apple-darwin",
                    "windows" => "pc-windows-msvc",
                    _ => return Err("Unsupported server platform.".into()),
                };
                let ext = if os == "windows" { "zip" } else { "gz" };
                (
                    format!("rust-analyzer-{arch}-{target}.{ext}"),
                    if os == "windows" {
                        "rust-analyzer.exe".into()
                    } else {
                        "rust-analyzer".into()
                    },
                )
            }
            ("clangd", "linux" | "macos" | "windows", "x86_64")
            | ("clangd", "macos", "aarch64") => {
                let platform = match os {
                    "linux" => "linux",
                    "macos" => "mac",
                    _ => "windows",
                };
                let exe = if os == "windows" {
                    "clangd.exe"
                } else {
                    "clangd"
                };
                (
                    format!("clangd-{platform}-{version}.zip"),
                    format!("clangd_{version}/bin/{exe}"),
                )
            }
            ("ty" | "ruff" | "tombi", "linux" | "macos" | "windows", "x86_64" | "aarch64") => {
                let target = match os {
                    "linux" if id == "tombi" => "unknown-linux-musl",
                    "linux" => "unknown-linux-gnu",
                    "macos" => "apple-darwin",
                    _ => "pc-windows-msvc",
                };
                let stem = if id == "tombi" {
                    format!(
                        "tombi-cli-{}-{arch}-{target}",
                        version.trim_start_matches('v')
                    )
                } else {
                    format!("{id}-{arch}-{target}")
                };
                let ext = if os == "windows" { "zip" } else { "tar.gz" };
                let exe = if os == "windows" {
                    format!("{id}.exe")
                } else {
                    id.into()
                };
                (
                    format!("{stem}.{ext}"),
                    if os == "windows" {
                        exe
                    } else {
                        format!("{stem}/{exe}")
                    },
                )
            }
            ("lua-language-server", "linux" | "macos" | "windows", "x86_64" | "aarch64")
                if !(os == "windows" && arch == "aarch64") =>
            {
                let platform = match os {
                    "linux" => "linux",
                    "macos" => "darwin",
                    _ => "win32",
                };
                let cpu = if arch == "x86_64" { "x64" } else { "arm64" };
                let ext = if os == "windows" { "zip" } else { "tar.gz" };
                (
                    format!("lua-language-server-{version}-{platform}-{cpu}.{ext}"),
                    if os == "windows" {
                        "bin/lua-language-server.exe".into()
                    } else {
                        "bin/lua-language-server".into()
                    },
                )
            }
            _ => return Err(
                "No managed release supports this OS/architecture. Configure an external server."
                    .into(),
            ),
        };
        if !safe_relative(Path::new(&name))
            || Path::new(&name).components().count() != 1
            || !safe_relative(Path::new(&executable))
        {
            return Err("Unsafe release asset name or executable path.".into());
        }
        let assets = value["assets"]
            .as_array()
            .ok_or("Missing release assets.")?;
        let matching: Vec<_> = assets
            .iter()
            .filter(|a| a["name"].as_str() == Some(&name))
            .collect();
        if matching.len() != 1 {
            return Err(format!("Release does not contain exactly one {name}."));
        }
        let asset = matching[0];
        let digest = asset["digest"]
            .as_str()
            .and_then(|s| s.strip_prefix("sha256:"))
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("Release asset has no SHA-256 digest; refusing an unverified install.")?
            .to_ascii_lowercase();
        let url = asset["browser_download_url"]
            .as_str()
            .ok_or("Missing download URL.")?
            .to_owned();
        let prefix = format!("https://github.com/{repository}/releases/download/");
        if !url.starts_with(&prefix)
            || !url.ends_with(&format!("/{name}"))
            || url.contains(['?', '#'])
        {
            return Err("Unexpected server download URL.".into());
        }
        let size = asset["size"]
            .as_u64()
            .filter(|&n| n > 0 && n <= DOWNLOAD_LIMIT)
            .ok_or("Invalid server archive size.")?;
        Ok(Self {
            version,
            url,
            digest,
            size,
            executable: executable.into(),
            gzip: name.ends_with(".gz"),
            tar: name.ends_with(".tar.gz"),
            probe: true,
        })
    }
}
fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        && !path.to_string_lossy().contains(['\\', ':'])
}
fn unpack(
    archive: &Path,
    destination: &Path,
    package: &Package,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if package.tar {
        let input = flate2::read::GzDecoder::new(File::open(archive).map_err(error)?);
        let mut archive = tar::Archive::new(input);
        let mut remaining = EXPANDED_LIMIT;
        let mut paths = std::collections::BTreeSet::new();
        for entry in archive.entries().map_err(error)?.raw(true) {
            check_cancel(cancel)?;
            let mut entry = entry.map_err(error)?;
            let path = entry.path().map_err(error)?.into_owned();
            let kind = entry.header().entry_type();
            if !safe_relative(&path)
                || !(kind.is_file() || kind.is_dir())
                || (kind.is_dir() && entry.size() != 0)
                || path.as_os_str().len() > 4096
                || !paths.insert(path.clone())
                || paths.len() > 20_000
            {
                return Err(
                    "Unsafe path, duplicate entry or unsupported type in server archive.".into(),
                );
            }
            let target = destination.join(path);
            if kind.is_dir() {
                fs::create_dir_all(target).map_err(error)?;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(error)?;
                }
                remaining -= copy_bounded(&mut entry, &target, remaining, cancel)?;
            }
        }
        // Finish decoding to validate the gzip checksum even after TAR's terminator.
        let mut input = archive.into_inner();
        let mut tail = std::io::Read::by_ref(&mut input).take(EXPANDED_LIMIT + 1);
        if std::io::copy(&mut tail, &mut std::io::sink()).map_err(error)? > remaining {
            return Err("Expanded server archive is too large.".into());
        }
        return Ok(());
    }
    if package.gzip {
        let mut input = flate2::read::GzDecoder::new(File::open(archive).map_err(error)?);
        return copy_bounded(
            &mut input,
            &destination.join(&package.executable),
            EXPANDED_LIMIT,
            cancel,
        )
        .map(|_| ());
    }
    let expected = validate_zip_directory(archive)?;
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(error)?).map_err(error)?;
    if zip.len() != expected {
        return Err("Duplicate or inconsistent ZIP directory entries.".into());
    }
    if zip.len() > 20_000 {
        return Err("Server archive has too many entries.".into());
    }
    let mut remaining = EXPANDED_LIMIT;
    for i in 0..zip.len() {
        check_cancel(cancel)?;
        let mut entry = zip.by_index(i).map_err(error)?;
        let path = PathBuf::from(entry.name().map_err(error)?.trim_end_matches('/'));
        if !safe_relative(&path) || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("Unsafe path or symbolic link in server archive.".into());
        }
        let target = destination.join(path);
        if entry.is_dir() {
            fs::create_dir_all(target).map_err(error)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(error)?;
        }
        let bytes = copy_bounded(&mut entry, &target, remaining, cancel)?;
        remaining -= bytes;
    }
    Ok(())
}
// zip's reader indexes entries by name and can hide duplicate central-directory
// records. Validate the raw directory before allowing it to deduplicate anything.
// Managed packages are bounded ordinary ZIPs; split/ZIP64 archives are refused.
fn validate_zip_directory(path: &Path) -> Result<usize, String> {
    let mut file = File::open(path).map_err(error)?;
    let length = file.metadata().map_err(error)?.len();
    let tail_length = length.min(65557) as usize;
    file.seek(SeekFrom::End(-(tail_length as i64)))
        .map_err(error)?;
    let mut tail = vec![0; tail_length];
    file.read_exact(&mut tail).map_err(error)?;
    let end = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            tail[i..].starts_with(b"PK\x05\x06")
                && i + 22 + u16::from_le_bytes([tail[i + 20], tail[i + 21]]) as usize == tail.len()
        })
        .ok_or("Missing ZIP directory terminator.")?;
    let u16_at = |n| u16::from_le_bytes([tail[end + n], tail[end + n + 1]]) as usize;
    let u32_at = |n| u32::from_le_bytes(tail[end + n..end + n + 4].try_into().unwrap()) as u64;
    let count = u16_at(10);
    let size = u32_at(12);
    let start = u32_at(16);
    if u16_at(4) != 0
        || u16_at(6) != 0
        || u16_at(8) != count
        || count > 20_000
        || start.checked_add(size) != Some(length - tail_length as u64 + end as u64)
    {
        return Err("Unsupported or inconsistent ZIP directory.".into());
    }
    file.seek(SeekFrom::Start(start)).map_err(error)?;
    let mut consumed = 0u64;
    let mut names = std::collections::BTreeSet::new();
    let mut name_bytes = 0;
    for _ in 0..count {
        let mut header = [0; 46];
        file.read_exact(&mut header).map_err(error)?;
        if &header[..4] != b"PK\x01\x02" {
            return Err("Invalid ZIP directory record.".into());
        }
        let field = |n| u16::from_le_bytes([header[n], header[n + 1]]) as usize;
        let n = field(28);
        let skip = field(30) + field(32);
        name_bytes += n;
        consumed += 46 + n as u64 + skip as u64;
        if n == 0 || n > 4096 || name_bytes > 4 * 1024 * 1024 || consumed > size {
            return Err("ZIP directory exceeds its limits.".into());
        }
        let mut name = vec![0; n];
        file.read_exact(&mut name).map_err(error)?;
        if !names.insert(name) {
            return Err("Duplicate ZIP archive entry.".into());
        }
        file.seek(SeekFrom::Current(skip as i64)).map_err(error)?;
    }
    if consumed != size {
        return Err("Inconsistent ZIP entry count.".into());
    }
    Ok(count)
}
fn copy_bounded(
    input: &mut impl Read,
    path: &Path,
    limit: u64,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(error)?;
    let mut count = 0;
    let mut bytes = [0; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let n = input.read(&mut bytes).map_err(error)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > limit {
            return Err("Expanded server archive is too large.".into());
        }
        out.write_all(&bytes[..n]).map_err(error)?;
    }
    out.sync_all().map_err(error)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn package(path: &Path, executable: &str, gzip: bool) -> Package {
        let bytes = fs::read(path).unwrap();
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Package {
            version: "fixture-1".into(),
            url: "https://github.com/rust-lang/rust-analyzer/releases/download/fixture/package.gz"
                .into(),
            digest,
            size: bytes.len() as u64,
            executable: executable.into(),
            gzip,
            tar: false,
            probe: false,
        }
    }
    fn gzip(path: &Path, data: &[u8]) {
        let mut encoder = flate2::write::GzEncoder::new(
            File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap();
    }
    fn zip(path: &Path, entries: &[(&str, &str)], symlink: bool) {
        let mut writer = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            writer.start_file(name, options).unwrap();
            writer.write_all(data.as_bytes()).unwrap();
        }
        if symlink {
            writer.add_symlink("link", "/etc/passwd", options).unwrap();
        }
        writer.finish().unwrap();
    }
    #[test]
    fn verified_gzip_install_is_atomic_and_failed_upgrade_keeps_current() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(dir.path().join("servers"));
        let archive = dir.path().join("server.gz");
        gzip(&archive, b"fixture executable");
        let p = package(&archive, "rust-analyzer", true);
        manager
            .activate(
                "rust-analyzer",
                &p,
                &archive,
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        let original = manager
            .read("rust-analyzer")
            .unwrap()
            .unwrap()
            .managed
            .unwrap();
        let (launch, lease) = manager.launch("rust-analyzer").unwrap();
        assert_eq!(fs::read(&launch.executable).unwrap(), b"fixture executable");
        drop(lease);
        fs::write(&archive, b"corrupt upgrade").unwrap();
        assert!(manager
            .activate(
                "rust-analyzer",
                &p,
                &archive,
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err());
        let current = manager
            .read("rust-analyzer")
            .unwrap()
            .unwrap()
            .managed
            .unwrap();
        assert_eq!(current.directory, original.directory);
        assert!(manager.launch("rust-analyzer").is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn an_incompatible_update_preserves_the_previous_runnable_installation() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(dir.path().join("servers"));
        let archive = dir.path().join("server.gz");
        gzip(&archive, b"#!/bin/sh\nprintf 'fixture version\\n'\n");
        let mut first = package(&archive, "rust-analyzer", true);
        first.probe = true;
        manager
            .activate(
                "rust-analyzer",
                &first,
                &archive,
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        let previous = manager
            .read("rust-analyzer")
            .unwrap()
            .unwrap()
            .managed
            .unwrap()
            .directory;
        gzip(&archive, b"#!/bin/sh\nexit 9\n");
        let mut update = package(&archive, "rust-analyzer", true);
        update.probe = true;
        assert!(manager
            .activate(
                "rust-analyzer",
                &update,
                &archive,
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err());
        assert_eq!(
            manager
                .read("rust-analyzer")
                .unwrap()
                .unwrap()
                .managed
                .unwrap()
                .directory,
            previous
        );
        let (launch, lease) = manager.launch("rust-analyzer").unwrap();
        probe_executable(
            &launch.executable,
            &AtomicBool::new(false),
            Duration::from_secs(1),
        )
        .unwrap();
        drop(lease);
    }
    #[cfg(unix)]
    #[test]
    fn failed_receipt_publication_preserves_existing_and_candidate_files() {
        use std::os::unix::fs::PermissionsExt;
        if rustix::process::geteuid().is_root() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(dir.path().join("servers"));
        let archive = dir.path().join("server.gz");
        gzip(&archive, b"previous");
        let first = package(&archive, "rust-analyzer", true);
        manager
            .activate(
                "rust-analyzer",
                &first,
                &archive,
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        let previous = manager
            .read("rust-analyzer")
            .unwrap()
            .unwrap()
            .managed
            .unwrap()
            .directory;
        gzip(&archive, b"candidate");
        let update = package(&archive, "rust-analyzer", true);
        let server = manager.root().join("rust-analyzer");
        let result = manager.activate(
            "rust-analyzer",
            &update,
            &archive,
            &AtomicBool::new(false),
            |message| {
                if message == "Activating…" {
                    fs::set_permissions(&server, fs::Permissions::from_mode(0o500)).unwrap();
                }
            },
        );
        fs::set_permissions(&server, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(
            manager
                .read("rust-analyzer")
                .unwrap()
                .unwrap()
                .managed
                .unwrap()
                .directory,
            previous
        );
        assert_eq!(
            fs::read(server.join(&previous).join("rust-analyzer")).unwrap(),
            b"previous"
        );
        let installations = fs::read_dir(&server)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("install-"))
            .count();
        assert_eq!(installations, 2);
    }
    #[cfg(unix)]
    #[test]
    fn version_checks_can_be_cancelled_and_hung_candidates_are_terminated() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hang");
        fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(probe_executable(&path, &AtomicBool::new(true), Duration::from_secs(1)).is_err());
        let start = Instant::now();
        assert!(
            probe_executable(&path, &AtomicBool::new(false), Duration::from_millis(50)).is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn cancellation_at_activation_keeps_the_previous_receipt_and_cleans_the_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(dir.path().join("servers"));
        let archive = dir.path().join("server.gz");
        gzip(&archive, b"previous");
        let first = package(&archive, "rust-analyzer", true);
        manager
            .activate(
                "rust-analyzer",
                &first,
                &archive,
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        let previous = manager
            .read("rust-analyzer")
            .unwrap()
            .unwrap()
            .managed
            .unwrap()
            .directory;
        gzip(&archive, b"candidate");
        let update = package(&archive, "rust-analyzer", true);
        let cancel = AtomicBool::new(false);
        assert!(manager
            .activate("rust-analyzer", &update, &archive, &cancel, |message| {
                if message == "Activating…" {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
            .is_err());
        assert_eq!(
            manager
                .read("rust-analyzer")
                .unwrap()
                .unwrap()
                .managed
                .unwrap()
                .directory,
            previous
        );
        assert_eq!(
            fs::read_dir(manager.root().join("rust-analyzer"))
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("install-"))
                .count(),
            1
        );
    }
    #[test]
    fn cancelled_install_leaves_no_registration_or_installation() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(dir.path().join("servers"));
        let archive = dir.path().join("server.gz");
        gzip(&archive, b"server");
        let p = package(&archive, "rust-analyzer", true);
        assert!(manager
            .activate(
                "rust-analyzer",
                &p,
                &archive,
                &AtomicBool::new(true),
                |_| {}
            )
            .is_err());
        assert!(manager.read("rust-analyzer").unwrap().is_none());
    }
    #[test]
    fn unsafe_zip_paths_symlinks_duplicates_and_missing_executable_are_rejected() {
        for (entries, link) in [
            (vec![("../escape", "x")], false),
            (vec![("/absolute", "x")], false),
            (vec![("a\\b", "x")], false),
            (vec![("C:evil", "x")], false),
            (vec![("ok", "x")], true),
            (vec![("other", "x")], false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let manager = Manager::new(dir.path().join("servers"));
            let archive = dir.path().join("server.zip");
            zip(&archive, &entries, link);
            let p = package(&archive, "rust-analyzer", false);
            assert!(
                manager
                    .activate(
                        "rust-analyzer",
                        &p,
                        &archive,
                        &AtomicBool::new(false),
                        |_| {}
                    )
                    .is_err(),
                "{entries:?} {link}"
            );
            assert!(manager.read("rust-analyzer").unwrap().is_none());
            assert!(!dir.path().join("escape").exists());
        }
    }
    #[test]
    fn zip_install_preserves_supporting_files_and_removes_only_owned_directories() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(dir.path().join("servers"));
        let archive = dir.path().join("server.zip");
        zip(
            &archive,
            &[
                ("clangd_1/bin/clangd", "executable"),
                ("clangd_1/lib/clang/resources", "resources"),
            ],
            false,
        );
        let p = package(&archive, "clangd_1/bin/clangd", false);
        manager
            .activate("clangd", &p, &archive, &AtomicBool::new(false), |_| {})
            .unwrap();
        let (launch, lease) = manager.launch("clangd").unwrap();
        assert!(launch
            .executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("lib/clang/resources")
            .is_file());
        drop(lease);
        fs::write(manager.root().join("clangd/user-file"), "keep").unwrap();
        manager.remove("clangd").unwrap();
        assert!(manager.root().join("clangd/user-file").exists());
        assert!(!launch.executable.exists());
    }
    #[test]
    fn duplicate_zip_names_are_refused_without_publishing_a_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("server.zip");
        zip(&archive, &[("aa", "x"), ("bb", "y")], false);
        let mut bytes = fs::read(&archive).unwrap();
        for n in 0..bytes.len() - 1 {
            if &bytes[n..n + 2] == b"bb" {
                bytes[n..n + 2].copy_from_slice(b"aa");
            }
        }
        fs::write(&archive, bytes).unwrap();
        let p = package(&archive, "aa", false);
        let manager = Manager::new(dir.path().join("servers"));
        assert!(manager
            .activate(
                "rust-analyzer",
                &p,
                &archive,
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err());
        assert!(manager.read("rust-analyzer").unwrap().is_none());
    }
    #[test]
    fn expanded_data_is_bounded_and_partial_files_stay_in_staging() {
        let dir = tempfile::tempdir().unwrap();
        let mut data = &b"12345"[..];
        assert!(copy_bounded(
            &mut data,
            &dir.path().join("partial"),
            4,
            &AtomicBool::new(false)
        )
        .is_err());
    }
    fn release() -> serde_json::Value {
        serde_json::json!({"tag_name":"fixture","draft":false,"prerelease":false,"assets":[{"name":"rust-analyzer-x86_64-unknown-linux-gnu.gz","browser_download_url":"https://github.com/rust-lang/rust-analyzer/releases/download/fixture/rust-analyzer-x86_64-unknown-linux-gnu.gz","size":123,"digest":format!("sha256:{}","a".repeat(64))}]})
    }
    #[test]
    fn release_metadata_must_have_exact_asset_hash_origin_and_supported_platform() {
        let good = release();
        assert!(Package::resolve(
            "rust-analyzer",
            "rust-lang/rust-analyzer",
            &good,
            "linux",
            "x86_64"
        )
        .is_ok());
        for key in ["digest", "size", "browser_download_url"] {
            let mut bad = good.clone();
            bad["assets"][0].as_object_mut().unwrap().remove(key);
            assert!(Package::resolve(
                "rust-analyzer",
                "rust-lang/rust-analyzer",
                &bad,
                "linux",
                "x86_64"
            )
            .is_err());
        }
        let mut bad = good.clone();
        bad["assets"][0]["browser_download_url"] = serde_json::json!("https://evil.test/server.gz");
        assert!(Package::resolve(
            "rust-analyzer",
            "rust-lang/rust-analyzer",
            &bad,
            "linux",
            "x86_64"
        )
        .is_err());
        let mut bad = good.clone();
        bad["assets"]
            .as_array_mut()
            .unwrap()
            .push(good["assets"][0].clone());
        assert!(Package::resolve(
            "rust-analyzer",
            "rust-lang/rust-analyzer",
            &bad,
            "linux",
            "x86_64"
        )
        .is_err());
        let mut bad = good.clone();
        bad["prerelease"] = serde_json::json!(true);
        assert!(Package::resolve(
            "rust-analyzer",
            "rust-lang/rust-analyzer",
            &bad,
            "linux",
            "x86_64"
        )
        .is_err());
        assert!(Package::resolve("clangd", "clangd/clangd", &good, "linux", "aarch64").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn manager_rejects_symlink_server_directories_and_locks() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("servers");
        fs::create_dir(&root).unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("fixture")).unwrap();
        let manager = Manager::new(root.clone());
        assert!(manager.remove("fixture").is_err());
        fs::remove_file(root.join("fixture")).unwrap();
        fs::create_dir(root.join("fixture")).unwrap();
        symlink(outside.join("lock"), root.join("fixture/lock")).unwrap();
        assert!(manager.remove("fixture").is_err());
    }
    #[test]
    fn tar_installs_preserve_runtime_files_and_default_launch_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("server.tar.gz");
        tar_fixture(
            &archive,
            &[
                (
                    "bin/lua-language-server",
                    tar::EntryType::Regular,
                    b"executable",
                ),
                ("main.lua", tar::EntryType::Regular, b"runtime"),
            ],
        );
        let mut p = package(&archive, "bin/lua-language-server", true);
        p.tar = true;
        let manager = Manager::new(dir.path().join("servers"));
        manager
            .activate(
                "lua-language-server",
                &p,
                &archive,
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        let (launch, lease) = manager.launch("lua-language-server").unwrap();
        assert_eq!(
            fs::read(
                launch
                    .executable
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("main.lua")
            )
            .unwrap(),
            b"runtime"
        );
        drop(lease);
        let mut registration = manager.read("lua-language-server").unwrap().unwrap();
        registration.arguments = Some(vec!["--custom".into()]);
        registration
            .environment
            .insert("EXAMPLE".into(), "value".into());
        manager
            .configure("lua-language-server", registration)
            .unwrap();
        manager
            .activate(
                "lua-language-server",
                &p,
                &archive,
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        let (launch, _) = manager.launch("lua-language-server").unwrap();
        assert_eq!(launch.arguments, ["--custom"]);
        assert_eq!(
            manager
                .read("lua-language-server")
                .unwrap()
                .unwrap()
                .environment["EXAMPLE"],
            "value"
        );
    }
    fn tar_fixture(path: &Path, entries: &[(&str, tar::EntryType, &[u8])]) {
        let output = flate2::write::GzEncoder::new(
            File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        let mut builder = tar::Builder::new(output);
        for (name, kind, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(data.len() as u64);
            header.set_entry_type(*kind);
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }
    #[test]
    fn tar_rejects_traversal_links_duplicate_names_and_metadata_entries() {
        for (name, kind) in [
            ("../escape", tar::EntryType::Regular),
            ("/escape", tar::EntryType::Regular),
            ("a\\b", tar::EntryType::Regular),
            ("C:evil", tar::EntryType::Regular),
            ("server", tar::EntryType::Symlink),
            ("server", tar::EntryType::Link),
            ("server", tar::EntryType::Fifo),
            ("server", tar::EntryType::GNULongName),
            ("server", tar::EntryType::XHeader),
            ("server", tar::EntryType::Directory),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let archive = dir.path().join("server.tar.gz");
            tar_fixture(&archive, &[(name, kind, b"x")]);
            let mut p = package(&archive, "server", true);
            p.tar = true;
            let manager = Manager::new(dir.path().join("servers"));
            assert!(
                manager
                    .activate("ty", &p, &archive, &AtomicBool::new(false), |_| {})
                    .is_err(),
                "{name} {kind:?}"
            );
            assert!(manager.read("ty").unwrap().is_none());
            assert!(!dir.path().join("escape").exists());
        }
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("server.tar.gz");
        tar_fixture(
            &archive,
            &[
                ("server", tar::EntryType::Regular, b"one"),
                ("server", tar::EntryType::Regular, b"two"),
            ],
        );
        let mut p = package(&archive, "server", true);
        p.tar = true;
        assert!(unpack(
            &archive,
            &dir.path().join("out"),
            &p,
            &AtomicBool::new(false)
        )
        .is_err());
    }
    #[test]
    fn all_presets_validate_and_keep_companion_features_separate() {
        let entries = catalog();
        let mut ids = std::collections::BTreeSet::new();
        for entry in &entries {
            assert!(ids.insert(&entry.id));
            validate(&preset(&entry.id)).unwrap();
        }
        assert_eq!(
            entries.iter().filter(|e| !e.repository.is_empty()).count(),
            6
        );
        assert_eq!(default_arguments("ty"), ["server"]);
        assert!(preset("ty").arguments.is_none());
        assert!(preset("ty").features.completion);
        assert!(!preset("ty").features.formatting);
        assert!(!preset("ruff").features.hover);
        assert!(preset("ruff").features.formatting);
        assert!(preset("ruff").features.diagnostics);
        assert!(preset("ty").priority > preset("ruff").priority);
        assert_eq!(
            preset("lua-language-server").configuration,
            serde_json::json!({"Lua":{}})
        );
    }

    #[test]
    fn native_catalog_resolves_published_release_assets_for_supported_platforms() {
        for (id, fixture) in [
            ("ty", include_str!("../../tests/support/releases/ty.json")),
            (
                "ruff",
                include_str!("../../tests/support/releases/ruff.json"),
            ),
            (
                "lua-language-server",
                include_str!("../../tests/support/releases/lua-language-server.json"),
            ),
            (
                "tombi",
                include_str!("../../tests/support/releases/tombi.json"),
            ),
        ] {
            let release = serde_json::from_str(fixture).unwrap();
            let entry = catalog().into_iter().find(|e| e.id == id).unwrap();
            for os in ["linux", "macos", "windows"] {
                for arch in ["x86_64", "aarch64"] {
                    let package = Package::resolve(id, &entry.repository, &release, os, arch);
                    if id == "lua-language-server" && os == "windows" && arch == "aarch64" {
                        assert!(package.is_err());
                        continue;
                    }
                    let package = package.unwrap_or_else(|e| panic!("{id} {os} {arch}: {e}"));
                    assert_eq!(package.tar, os != "windows");
                    assert_eq!(package.gzip, os != "windows");
                    assert!(safe_relative(&package.executable));
                    if os == "windows" && id != "lua-language-server" {
                        assert_eq!(package.executable, PathBuf::from(format!("{id}.exe")));
                    }
                }
            }
        }
    }
    #[test]
    #[ignore = "requires verified upstream archives and release JSON in HEDIT_LSP_ASSET_DIR"]
    fn live_cached_native_installs_start_and_serve_language_requests() {
        use super::super::{
            client::{Client, ClientSettings, Processes},
            text,
        };
        let assets =
            PathBuf::from(std::env::var_os("HEDIT_LSP_ASSET_DIR").expect("HEDIT_LSP_ASSET_DIR"));
        for (id, metadata, filename, language, source, method) in [
            (
                "ty",
                "astral-sh-ty.json",
                "main.py",
                "python",
                "answer: int = 42\nprint(answer)\n",
                "textDocument/hover",
            ),
            (
                "ruff",
                "astral-sh-ruff.json",
                "main.py",
                "python",
                "answer=42\n",
                "textDocument/formatting",
            ),
            (
                "lua-language-server",
                "LuaLS-lua-language-server.json",
                "main.lua",
                "lua",
                "local answer = 42\nprint(answer)\n",
                "textDocument/hover",
            ),
            (
                "tombi",
                "tombi-toml-tombi.json",
                "example.toml",
                "toml",
                "answer=42\n",
                "textDocument/formatting",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            let manager = Manager::new(root.join("servers"));
            let entry = catalog().into_iter().find(|e| e.id == id).unwrap();
            let release =
                serde_json::from_slice(&fs::read(assets.join(metadata)).unwrap()).unwrap();
            let package = Package::resolve(
                id,
                &entry.repository,
                &release,
                std::env::consts::OS,
                std::env::consts::ARCH,
            )
            .unwrap();
            let archive = assets.join(package.url.rsplit('/').next().unwrap());
            manager
                .activate(id, &package, &archive, &AtomicBool::new(false), |s| {
                    eprintln!("{id}: {s}")
                })
                .unwrap();
            let r = manager.read(id).unwrap().unwrap();
            let (launch, lease) = manager.launch(id).unwrap();
            eprintln!("{id}: {:?} {:?}", launch.executable, launch.arguments);
            let settings = ClientSettings {
                configuration: r.configuration,
                initialization_options: r.initialization_options.unwrap(),
                environment: r.environment,
                section: id.into(),
            };
            let mut client = Client::start_owned(
                launch,
                lease,
                &root,
                settings,
                &AtomicBool::new(false),
                std::sync::Arc::new(Processes::default()),
            )
            .unwrap();
            let file = root.join(filename);
            fs::write(&file, source).unwrap();
            let uri = text::uri(&file).unwrap();
            client.notify("textDocument/didOpen", serde_json::json!({"textDocument":{"uri":uri,"languageId":language,"version":1,"text":source}})).unwrap();
            let position = client
                .encoding
                .position(source, source.find("answer").unwrap() + 2)
                .unwrap();
            let until = Instant::now() + Duration::from_secs(20);
            loop {
                let result = client.request(method, serde_json::json!({"textDocument":{"uri":uri},"position":position,"options":{"tabSize":4,"insertSpaces":true}}), &AtomicBool::new(false), Duration::from_secs(10));
                if result.as_ref().is_ok_and(|v| {
                    !v.is_null()
                        && (id != "lua-language-server"
                            || v["contents"]["value"]
                                .as_str()
                                .is_some_and(|s| s.contains("answer")))
                }) {
                    eprintln!("{id}: {method}: {}", result.unwrap());
                    break;
                }
                assert!(
                    Instant::now() < until,
                    "{id}: {result:?} logs {:?}",
                    client.logs.lock().unwrap()
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            client.stop();
            drop(client);
            manager.remove(id).unwrap();
            assert!(manager.list().unwrap().is_empty());
            assert!(file.is_file());
        }
    }
    #[cfg(unix)]
    #[test]
    fn path_detection_skips_relative_entries_and_nonexecutable_files() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(first.join("gopls"), "never run").unwrap();
        fs::set_permissions(first.join("gopls"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(second.join("gopls"), "never run").unwrap();
        fs::set_permissions(second.join("gopls"), fs::Permissions::from_mode(0o700)).unwrap();
        let path =
            std::env::join_paths([Path::new("."), Path::new("relative"), &first, &second]).unwrap();
        assert_eq!(detect_in("gopls", &path).unwrap(), second.join("gopls"));
        assert!(detect_in("missing", &path).is_err());
        assert!(detect("custom-id").is_err());
    }
    #[test]
    fn release_versions_cannot_escape_installation_paths() {
        let mut bad = release();
        bad["tag_name"] = serde_json::json!("v1/../../escape");
        for (id, repo) in [
            ("clangd", "clangd/clangd"),
            ("tombi", "tombi-toml/tombi"),
            ("lua-language-server", "LuaLS/lua-language-server"),
        ] {
            assert!(Package::resolve(id, repo, &bad, "linux", "x86_64")
                .unwrap_err()
                .contains("Unsafe release"));
        }
    }
    #[test]
    fn registrations_cannot_exceed_the_pretty_printed_storage_limit() {
        let mut registration = preset("ty");
        registration.environment = (0..128)
            .map(|i| (format!("VARIABLE_{i}"), "x".repeat(7800)))
            .collect();
        registration.arguments = Some((0..128).map(|_| "a".repeat(150)).collect());
        let size = serde_json::to_vec(&registration).unwrap().len();
        registration.configuration =
            serde_json::json!({"padding":"x".repeat(1024 * 1024 - size - 64)});
        assert!(serde_json::to_vec(&registration).unwrap().len() < 1024 * 1024);
        assert!(serde_json::to_vec_pretty(&registration).unwrap().len() > 1024 * 1024);
        assert!(validate(&registration).is_err());
    }
}
