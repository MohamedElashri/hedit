//! Bounded SSH setup; helper bytes come from this installation or its exact release.
use super::Config;
use hedit_remote::client::{SetupInfo, SETUP_VERSION};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LIMIT: u64 = 64 * 1024;
const PROBE: &str =
    "set -eu; printf 'HEDIT_SETUP_V1\\n'; id -u; uname -s; uname -m; printf '%s\\n' \"$HOME\"";

#[derive(Debug)]
struct Account {
    uid: u32,
    home: String,
    os: &'static str,
    arch: &'static str,
}
impl Account {
    fn parse(value: &str) -> Result<Self, String> {
        let lines: Vec<_> = value.lines().collect();
        if lines.len() != 5 || lines[0] != "HEDIT_SETUP_V1" {
            return Err("Cannot identify the SSH account. Ensure its shell does not write startup messages to stdout.".into());
        }
        let uid = lines[1]
            .parse::<u32>()
            .map_err(|_| "Invalid remote account ID.")?;
        if uid == 0 {
            return Err("Automatic SSH setup requires a non-root SSH account. Choose a user alias in your SSH configuration.".into());
        }
        let os = match lines[2] {
            "Linux" => "linux",
            "Darwin" => "macos",
            _ => return Err("SSH helpers support Linux and macOS hosts.".into()),
        };
        let arch = match lines[3] {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            _ => return Err("SSH helpers support x86-64 and ARM64 hosts.".into()),
        };
        absolute(lines[4])?;
        if lines[4] == "/" {
            return Err("The remote account needs its own home directory.".into());
        }
        Ok(Self {
            uid,
            home: lines[4].trim_end_matches('/').into(),
            os,
            arch,
        })
    }
    fn helper(&self) -> String {
        format!(
            "{}/.local/share/hedit/remote/{VERSION}/{}-{}/hedit-remote",
            self.home, self.os, self.arch
        )
    }
    fn asset(&self) -> String {
        if self.os == "macos" {
            format!("hedit-remote-{VERSION}-macos-universal")
        } else {
            format!("hedit-remote-{VERSION}-linux-{}", self.arch)
        }
    }
    fn resolve(&self, mut config: Config, output: &str) -> Result<Config, String> {
        let info: SetupInfo =
            serde_json::from_str(output).map_err(|e| format!("Invalid SSH setup response: {e}"))?;
        let root = if config.root.is_empty() {
            self.home.clone()
        } else {
            config.root.clone()
        };
        let key = Sha256::digest(root.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let state = if config.target.state.is_empty() {
            format!("{}/.local/state/hedit/remote/{key}", self.home)
        } else {
            config.target.state.clone()
        };
        if info.version != VERSION
            || info.uid != self.uid
            || info.home != self.home
            || info.root != root
            || info.helper != self.helper()
            || info.state != state
        {
            return Err("The SSH setup response did not match the requested account, version, or workspace.".into());
        }
        config.root = info.root;
        config.target.helper = info.helper;
        config.target.state = info.state;
        config.validate()?;
        Ok(config)
    }
}

pub(super) fn absolute(value: &str) -> Result<(), String> {
    if !value.starts_with('/')
        || value.len() > 4096
        || value.contains(['\0', '\r', '\n'])
        || value.split('/').any(|c| c == ".." || c == ".")
    {
        return Err("Use an absolute remote path without parent components or line breaks.".into());
    }
    Ok(())
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Install into the SSH user's private home directory; never invokes sudo or a package manager.
pub fn prepare(
    config: Config,
    cancel: &AtomicBool,
    progress: impl Fn(&str),
) -> Result<Config, String> {
    config.validate()?;
    if !config.managed {
        return Ok(config);
    }
    if !config.root.is_empty() {
        absolute(&config.root)?;
    }
    if !config.target.state.is_empty() {
        absolute(&config.target.state)?;
    }
    progress("Connecting to SSH host…");
    let (status, output) = run(
        config
            .target
            .command_for(PROBE)
            .map_err(|e| e.to_string())?,
        None,
        cancel,
    )?;
    if !status.success() {
        return Err(output);
    }
    let account = Account::parse(output.trim_end_matches(['\r', '\n']))?;
    progress("Checking remote helper…");
    let helper = account.helper();
    // Only execute a cached helper below non-symlink, user-owned directories.
    // The Rust installer repeats descriptor-relative validation before writing.
    let cached = format!(
        r#"set -eu
uid=$(id -u)
test "$uid" = {uid} || exit 1
test "$HOME" = {home} || exit 1
path="$HOME"
for part in .local share hedit remote {version} {platform}; do
    test -d "$path" && test ! -L "$path" || exit 78
    test -n "$(find "$path" -prune -user "$uid" ! -perm -0020 ! -perm -0002)" || exit 78
    path="$path/$part"
done
test -d "$path" && test ! -L "$path" || exit 78
test -n "$(find "$path" -prune -type d -user "$uid" -perm 0700)" || exit 78
test -f {helper} && test -x {helper} && test ! -L {helper} || exit 78
test -n "$(find {helper} -prune -type f -user "$uid" -perm 0700)" || exit 78
test "$({helper} --version)" = {expected} || exit 78
test "$({helper} --setup-version)" = {setup_version} || exit 78
exec {helper} setup --workspace {workspace} --state {state}
"#,
        uid = account.uid,
        home = quote(&account.home),
        version = quote(VERSION),
        platform = quote(&format!("{}-{}", account.os, account.arch)),
        helper = quote(&helper),
        expected = quote(&format!("hedit-remote {VERSION}")),
        workspace = quote(&config.root),
        state = quote(&config.target.state),
        setup_version = SETUP_VERSION,
    );
    let (status, output) = run(
        config
            .target
            .command_for(&cached)
            .map_err(|e| e.to_string())?,
        None,
        cancel,
    )?;
    if status.success() {
        return account.resolve(config, &output);
    }
    if status.code() != Some(78) {
        return Err(output);
    }

    let mut temporary = tempfile::Builder::new();
    temporary.prefix("hedit-ssh-helper-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let download = temporary.tempdir().map_err(|e| e.to_string())?;
    let source = match bundled(&account, cancel)? {
        Some(path) => path,
        None => {
            progress("Downloading matching SSH helper…");
            let path = download.path().join(account.asset());
            crate::update::Client::default().download_helper(
                VERSION,
                &account.asset(),
                &path,
                cancel,
            )?;
            path
        }
    };
    progress("Installing helper for your remote user…");
    let command = format!(
        r#"set -eu
umask 077
uid=$(id -u)
test "$uid" != 0 && test "$uid" = {uid} || exit 1
test "$HOME" = {home} || exit 1
test -d "$HOME" && test ! -L "$HOME" || exit 1
test -n "$(find "$HOME" -prune -user "$uid" ! -perm -0020 ! -perm -0002)" || exit 1
stage=$(mktemp -d "$HOME/.hedit-setup.XXXXXXXX")
trap 'rm -f "$stage/hedit-remote"; rmdir "$stage"' 0
trap 'exit 1' 1 2 15
cat > "$stage/hedit-remote"
chmod 700 "$stage/hedit-remote"
test "$("$stage/hedit-remote" --version)" = {expected}
"$stage/hedit-remote" setup --workspace {workspace} --state {state}
"#,
        uid = account.uid,
        home = quote(&account.home),
        expected = quote(&format!("hedit-remote {VERSION}")),
        workspace = quote(&config.root),
        state = quote(&config.target.state),
    );
    let input = File::open(&source).map_err(|e| e.to_string())?;
    let size = input.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 || size > 64 * 1024 * 1024 {
        return Err("Invalid SSH helper size.".into());
    }
    let (status, output) = run(
        config
            .target
            .command_for(&command)
            .map_err(|e| e.to_string())?,
        Some(input),
        cancel,
    )?;
    if !status.success() {
        return Err(format!(
            "Could not install the SSH helper for this user. {output}"
        ));
    }
    progress("Opening remote workspace…");
    account.resolve(config, &output)
}

fn bundled(account: &Account, cancel: &AtomicBool) -> Result<Option<PathBuf>, String> {
    if account.os != std::env::consts::OS || account.arch != std::env::consts::ARCH {
        return Ok(None);
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = executable
        .parent()
        .ok_or("Cannot locate the editor installation.")?;
    let mut candidates = vec![parent.join("hedit-remote")];
    // Cargo's test executables live one directory below the development binaries.
    if cfg!(debug_assertions)
        && parent
            .file_name()
            .is_some_and(|s| s == "deps" || s == "examples")
    {
        if let Some(parent) = parent.parent() {
            candidates.push(parent.join("hedit-remote"));
        }
    }
    for path in candidates {
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|m| m.is_file() && m.len() <= 64 * 1024 * 1024)
        {
            let mut command = Command::new(&path);
            command.arg("--version");
            if let Ok((status, output)) = run(command, None, cancel) {
                if status.success() && output.trim() == format!("hedit-remote {VERSION}") {
                    return Ok(Some(path));
                }
            }
        }
    }
    Ok(None)
}

fn read(file: &File) -> Result<String, String> {
    let mut file = file.try_clone().map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut result = String::new();
    file.take(LIMIT + 1)
        .read_to_string(&mut result)
        .map_err(|e| e.to_string())?;
    if result.len() as u64 > LIMIT {
        return Err("SSH setup output exceeded its size limit.".into());
    }
    Ok(result)
}
fn run(
    mut command: Command,
    input: Option<File>,
    cancel: &AtomicBool,
) -> Result<(ExitStatus, String), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("SSH connection cancelled.".into());
    }
    let output = tempfile::tempfile().map_err(|e| e.to_string())?;
    let stderr = tempfile::tempfile().map_err(|e| e.to_string())?;
    command
        .stdin(input.map(Stdio::from).unwrap_or_else(Stdio::null))
        .stdout(output.try_clone().map_err(|e| e.to_string())?)
        .stderr(stderr.try_clone().map_err(|e| e.to_string())?);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        let cancelled = cancel.load(Ordering::Relaxed);
        let oversized = output.metadata().map_err(|e| e.to_string())?.len() > LIMIT
            || stderr.metadata().map_err(|e| e.to_string())?.len() > LIMIT;
        if cancelled || oversized || started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancelled {
                "SSH connection cancelled."
            } else if oversized {
                "SSH setup output exceeded its size limit."
            } else {
                "SSH setup timed out."
            }
            .into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            let text = if status.success() {
                read(&output)?
            } else {
                read(&stderr)?
            };
            return Ok((
                status,
                if text.trim().is_empty() && !status.success() {
                    format!("SSH setup failed ({status}). Check the account's home directory permissions.")
                } else {
                    text
                },
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifies_supported_accounts_and_refuses_root_and_malformed_probes() {
        let account = Account::parse("HEDIT_SETUP_V1\n1000\nLinux\naarch64\n/home/me").unwrap();
        assert_eq!(account.os, "linux");
        assert_eq!(account.arch, "aarch64");
        assert!(Account::parse("HEDIT_SETUP_V1\n0\nLinux\nx86_64\n/root")
            .unwrap_err()
            .contains("non-root"));
        for raw in [
            "login banner\nHEDIT_SETUP_V1\n1000\nLinux\nx86_64\n/home/me",
            "HEDIT_SETUP_V1\n1000\nWindows\nx86_64\n/home/me",
            "HEDIT_SETUP_V1\n1000\nLinux\nx86_64\n/home/me/../other",
        ] {
            assert!(Account::parse(raw).is_err());
        }
    }
    #[test]
    fn cancellation_prevents_starting_a_process() {
        assert!(
            run(Command::new("does-not-exist"), None, &AtomicBool::new(true))
                .unwrap_err()
                .contains("cancelled")
        );
    }
    #[test]
    fn setup_replies_cannot_change_account_version_workspace_or_install_location() {
        let account = Account::parse("HEDIT_SETUP_V1\n1000\nLinux\nx86_64\n/home/me").unwrap();
        let config = Config {
            target: hedit_remote::client::SshTarget {
                host: "devbox".into(),
                user: None,
                port: 0,
                config: None,
                helper: String::new(),
                state: String::new(),
            },
            root: String::new(),
            managed: true,
            takeover: false,
        };
        let key = Sha256::digest(b"/home/me")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let info = SetupInfo {
            version: VERSION.into(),
            uid: 1000,
            home: "/home/me".into(),
            root: "/home/me".into(),
            helper: account.helper(),
            state: format!("/home/me/.local/state/hedit/remote/{key}"),
        };
        let value = serde_json::to_value(info).unwrap();
        let resolved = account.resolve(config.clone(), &value.to_string()).unwrap();
        assert_eq!(resolved.root, "/home/me");
        assert!(resolved.managed);
        for (field, wrong) in [
            ("uid", serde_json::json!(0)),
            ("version", serde_json::json!("0.0.1")),
            ("home", serde_json::json!("/home/other")),
            ("root", serde_json::json!("/etc")),
            ("helper", serde_json::json!("/usr/bin/hedit-remote")),
            ("state", serde_json::json!("/tmp/state")),
        ] {
            let mut invalid = value.clone();
            invalid[field] = wrong;
            assert!(
                account
                    .resolve(config.clone(), &invalid.to_string())
                    .is_err(),
                "{field}"
            );
        }
        let mut custom = config.clone();
        custom.target.state = "/private/existing-recovery".into();
        let mut response = value.clone();
        response["state"] = serde_json::json!(custom.target.state);
        assert_eq!(
            account
                .resolve(custom, &response.to_string())
                .unwrap()
                .target
                .state,
            "/private/existing-recovery"
        );
    }
    #[cfg(unix)]
    #[test]
    fn setup_process_output_is_bounded_and_running_work_can_be_cancelled() {
        let mut oversized = Command::new("sh");
        oversized.args(["-c", "head -c 131072 /dev/zero"]);
        assert!(run(oversized, None, &AtomicBool::new(false))
            .unwrap_err()
            .contains("size limit"));
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 10"]);
        assert!(run(command, None, &cancel)
            .unwrap_err()
            .contains("cancelled"));
        thread.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
