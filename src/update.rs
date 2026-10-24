//! Manual GitHub release checks and verified Linux updates. Nothing runs on startup.
mod install;
// Updates install Linux packages; macOS apps are replaced by hand.
#[cfg(all(test, target_os = "linux"))]
mod tests;

pub use install::Installation;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const REPOSITORY: &str = "MohamedElashri/hedit";
pub const RELEASES_URL: &str = "https://github.com/MohamedElashri/hedit/releases";
const METADATA_LIMIT: u64 = 1024 * 1024;
const ARCHIVE_LIMIT: u64 = 512 * 1024 * 1024;
const CHECKSUM_LIMIT: u64 = 4096;

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Access {
    #[default]
    Public,
    GitHubCli,
}
#[derive(Clone, Debug, Deserialize)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
}
#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    pub notes: String,
    pub package: String,
    archive: Asset,
    checksum: Asset,
}
impl Release {
    pub fn newer_than(&self, current: &str) -> Result<bool, String> {
        let current = Version::parse(current).map_err(error)?;
        Ok(self.version.cmp_precedence(&current).is_gt())
    }
    pub fn url(&self) -> String {
        format!("{RELEASES_URL}/tag/{}", self.tag)
    }
    pub fn from_json(json: &str, arch: &str) -> Result<Self, String> {
        if json.len() as u64 > METADATA_LIMIT {
            return Err("Release metadata is too large.".into());
        }
        let api: ApiRelease = serde_json::from_str(json)
            .map_err(|e| format!("Invalid GitHub release response: {e}"))?;
        let version = api
            .tag_name
            .strip_prefix('v')
            .ok_or("Expected a release tag beginning with v.")?;
        let version =
            Version::parse(version).map_err(|e| format!("Invalid release version: {e}"))?;
        if api.draft || api.prerelease || !version.pre.is_empty() {
            return Err("Only published stable releases can be installed here.".into());
        }
        if !matches!(arch, "x86_64" | "aarch64") {
            return Err("In-app updates support Linux x86-64 and ARM64.".into());
        }
        let package = format!("hedit-{version}-linux-{arch}");
        let find = |name: &str, limit| -> Result<Asset, String> {
            let matches: Vec<_> = api
                .assets
                .iter()
                .filter(|asset| asset.name == name)
                .collect();
            if matches.len() != 1 {
                return Err(format!("The release needs exactly one {name} asset."));
            }
            let asset = matches[0];
            if asset.id == 0 || asset.size == 0 || asset.size > limit {
                return Err(format!("Invalid or oversized release asset: {name}"));
            }
            Ok(asset.clone())
        };
        let archive = find(&format!("{package}.tar.gz"), ARCHIVE_LIMIT)?;
        let checksum = find(&format!("{package}.tar.gz.sha256"), CHECKSUM_LIMIT)?;
        Ok(Self {
            version,
            tag: api.tag_name,
            notes: api.body.unwrap_or_default().chars().take(32_768).collect(),
            package,
            archive,
            checksum,
        })
    }
}

#[derive(Clone)]
pub struct Client {
    access: Access,
    curl: PathBuf,
    gh: PathBuf,
    tar: PathBuf,
}
impl Client {
    pub fn new(access: Access) -> Self {
        Self {
            access,
            curl: "curl".into(),
            gh: "gh".into(),
            tar: "tar".into(),
        }
    }
    fn curl(&self, url: &str, limit: u64) -> Command {
        let mut command = Command::new(&self.curl);
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
                "--max-redirs",
                "5",
                "--connect-timeout",
                "15",
                "--max-time",
                "300",
                "--max-filesize",
            ])
            .arg(limit.to_string())
            .args([
                "--user-agent",
                concat!("hedit/", env!("CARGO_PKG_VERSION")),
                url,
            ]);
        command
    }
    fn gh(&self, endpoint: &str, accept: &str) -> Command {
        let mut command = Command::new(&self.gh);
        command
            .args([
                "api",
                "--hostname",
                "github.com",
                endpoint,
                "--header",
                accept,
            ])
            .env("GH_PROMPT_DISABLED", "1")
            .env_remove("GH_DEBUG");
        command
    }
    pub fn check(&self) -> Result<Release, String> {
        if std::env::consts::OS != "linux" {
            return Err("In-app updates currently support Linux only.".into());
        }
        let output = tempfile::tempfile().map_err(error)?;
        let endpoint = format!("repos/{REPOSITORY}/releases/latest");
        let mut command = match self.access {
            Access::Public => self.curl(
                &format!("https://api.github.com/{endpoint}"),
                METADATA_LIMIT,
            ),
            Access::GitHubCli => self.gh(&endpoint, "Accept: application/vnd.github+json"),
        };
        run_tool(&mut command, &output, METADATA_LIMIT, Duration::from_secs(60))
            .map_err(|e| format!("Could not check GitHub releases. Check your connection; for private releases select GitHub CLI and run gh auth login.\n{e}"))?;
        Release::from_json(&read_text(&output, METADATA_LIMIT)?, std::env::consts::ARCH)
    }
    fn download(&self, release: &Release, asset: &Asset, destination: &Path) -> Result<(), String> {
        let output = File::create(destination).map_err(error)?;
        let mut command = match self.access {
            Access::Public => self.curl(
                &format!("{RELEASES_URL}/download/{}/{}", release.tag, asset.name),
                asset.size,
            ),
            Access::GitHubCli => self.gh(
                &format!("repos/{REPOSITORY}/releases/assets/{}", asset.id),
                "Accept: application/octet-stream",
            ),
        };
        run_tool(&mut command, &output, asset.size, Duration::from_secs(300))?;
        if output.metadata().map_err(error)?.len() != asset.size {
            return Err(format!("Incomplete download: {}", asset.name));
        }
        output.sync_all().map_err(error)
    }
    pub fn install(
        &self,
        release: &Release,
        installation: &Installation,
        progress: impl Fn(&'static str),
    ) -> Result<(), String> {
        if !release.newer_than(env!("CARGO_PKG_VERSION"))? {
            return Err("This release is not newer than the running version.".into());
        }
        let _lock = installation.lock()?;
        let work = tempfile::Builder::new()
            .prefix("hedit-download-")
            .tempdir()
            .map_err(error)?;
        let archive = work.path().join(&release.archive.name);
        let checksum = work.path().join(&release.checksum.name);
        progress("Downloading release and checksum…");
        self.download(release, &release.archive, &archive)?;
        self.download(release, &release.checksum, &checksum)?;
        progress("Verifying SHA-256 checksum…");
        verify_checksum(
            &archive,
            &fs::read_to_string(checksum).map_err(error)?,
            &release.archive.name,
        )?;
        progress("Validating and unpacking the release…");
        let unpacked = work.path().join("unpacked");
        fs::create_dir(&unpacked).map_err(error)?;
        unpack(&self.tar, &archive, &unpacked, &release.package)?;
        let package = unpacked.join(&release.package);
        install::verify_binary(&package.join("hedit"), &release.version.to_string())?;
        progress("Installing update… Keep hedit open until this finishes.");
        installation.replace(&package)
    }
}

fn read_text(file: &File, limit: u64) -> Result<String, String> {
    let mut file = file.try_clone().map_err(error)?;
    file.seek(SeekFrom::Start(0)).map_err(error)?;
    let mut text = String::new();
    file.take(limit + 1)
        .read_to_string(&mut text)
        .map_err(error)?;
    if text.len() as u64 > limit {
        return Err("Tool output exceeded its size limit.".into());
    }
    Ok(text)
}
fn run_tool(
    command: &mut Command,
    output: &File,
    limit: u64,
    timeout: Duration,
) -> Result<(), String> {
    let stderr = tempfile::tempfile().map_err(error)?;
    command
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(error)?)
        .stderr(stderr.try_clone().map_err(error)?);
    // A just-written executable can briefly be "text file busy" while another
    // thread's fork still holds it open; retry as Cargo does.
    let mut attempts = 0;
    let mut child = loop {
        match command.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 20 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            result => {
                break result
                    .map_err(|e| format!("Could not start {:?}: {e}", command.get_program()))?
            }
        }
    };
    let started = Instant::now();
    loop {
        let oversized = output.metadata().map_err(error)?.len() > limit
            || stderr.metadata().map_err(error)?.len() > 64 * 1024;
        if oversized || started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if oversized {
                "Tool output exceeded its size limit."
            } else {
                "Update tool timed out."
            }
            .into());
        }
        if let Some(status) = child.try_wait().map_err(error)? {
            if status.success() {
                if output.metadata().map_err(error)?.len() > limit
                    || stderr.metadata().map_err(error)?.len() > 64 * 1024
                {
                    return Err("Tool output exceeded its size limit.".into());
                }
                return Ok(());
            }
            return Err(format!(
                "Tool failed ({status}): {}",
                read_text(&stderr, 64 * 1024)?.trim()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}
pub fn verify_checksum(archive: &Path, checksum: &str, name: &str) -> Result<(), String> {
    let line = checksum.trim_end_matches('\n');
    let (digest, filename) = line
        .split_once("  ")
        .ok_or("Invalid SHA-256 checksum file.")?;
    if filename != name || digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid SHA-256 checksum file.".into());
    }
    let mut input = File::open(archive).map_err(error)?;
    let mut hasher = Sha256::new();
    let mut bytes = [0; 64 * 1024];
    loop {
        let count = input.read(&mut bytes).map_err(error)?;
        if count == 0 {
            break;
        }
        hasher.update(&bytes[..count]);
    }
    if format!("{:x}", hasher.finalize()) != digest.to_ascii_lowercase() {
        return Err("Checksum mismatch; the existing installation was not changed.".into());
    }
    Ok(())
}

// GNU tar performs parsing/extraction. Preflight the fully resolved listing and
// reject links, special files, duplicates, unsafe names, and oversized archives.
fn unpack(tar: &Path, archive: &Path, destination: &Path, package: &str) -> Result<(), String> {
    let listing = tempfile::tempfile().map_err(error)?;
    let mut command = Command::new(tar);
    command
        .args([
            "--list",
            "--verbose",
            "--gzip",
            "--numeric-owner",
            "--absolute-names",
            "--quoting-style=escape",
            "--file",
        ])
        .arg(archive)
        .env("LC_ALL", "C")
        .env_remove("TAR_OPTIONS");
    run_tool(
        &mut command,
        &listing,
        METADATA_LIMIT,
        Duration::from_secs(60),
    )?;
    validate_listing(&read_text(&listing, METADATA_LIMIT)?, package)?;
    let output = tempfile::tempfile().map_err(error)?;
    let mut command = Command::new(tar);
    command
        .args([
            "--extract",
            "--gzip",
            "--no-same-owner",
            "--no-same-permissions",
            "--keep-old-files",
            "--delay-directory-restore",
            "--file",
        ])
        .arg(archive)
        .arg("--directory")
        .arg(destination)
        .env("LC_ALL", "C")
        .env_remove("TAR_OPTIONS");
    run_tool(&mut command, &output, 64 * 1024, Duration::from_secs(120))
}
fn validate_listing(listing: &str, package: &str) -> Result<(), String> {
    use std::collections::HashSet;
    let mut paths = HashSet::new();
    let mut size = 0u64;
    for line in listing.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 6 || !matches!(fields[0].as_bytes().first(), Some(b'-' | b'd')) {
            return Err("Archive contains a link, special file, or unsupported path.".into());
        }
        let path = fields[5].trim_end_matches('/');
        if !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_.-+".contains(&b))
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || !(path == package || path.starts_with(&format!("{package}/")))
            || !paths.insert(path.to_owned())
            || paths.len() > 10_000
        {
            return Err("Archive contains an unsafe or duplicate path.".into());
        }
        size = size
            .checked_add(fields[2].parse::<u64>().map_err(error)?)
            .ok_or("Archive size overflow.")?;
        if size > 1024 * 1024 * 1024 {
            return Err("Unpacked release exceeds 1 GiB.".into());
        }
    }
    if paths.is_empty() {
        return Err("Release archive is empty.".into());
    }
    Ok(())
}
