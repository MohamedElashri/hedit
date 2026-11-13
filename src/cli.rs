//! Small, native command line. No shell is used to open input.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Debug, Default)]
pub struct Options {
    pub remote: Option<crate::remote::Config>,
    pub session_dir: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    pub stdin: bool,
    pub new_buffer: bool,
    /// Keep the terminal waiting until the files are closed (`-w`), so hedit
    /// can be the editor for tools such as `git commit`.
    pub wait: bool,
    pub help: bool,
    pub version: bool,
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        let mut literal = false;
        let mut ssh = None;
        let mut root = None;
        let mut helper = None;
        let mut state = None;
        let mut user = None;
        let mut config = None;
        let mut port = 22;
        let mut takeover = false;
        while let Some(arg) = args.next() {
            if literal {
                options.files.push(arg.into());
                continue;
            }
            match arg.to_str() {
                Some("--") => literal = true,
                Some("--ssh") => {
                    ssh = Some(
                        args.next()
                            .ok_or("--ssh needs a host")?
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
                Some("--ssh-root") => {
                    root = Some(
                        args.next()
                            .ok_or("--ssh-root needs an absolute path")?
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
                Some("--ssh-helper") => {
                    helper = Some(
                        args.next()
                            .ok_or("--ssh-helper needs an absolute path")?
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
                Some("--ssh-state") => {
                    state = Some(
                        args.next()
                            .ok_or("--ssh-state needs an absolute path")?
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
                Some("--ssh-user") => {
                    user = Some(
                        args.next()
                            .ok_or("--ssh-user needs a user")?
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
                Some("--ssh-config") => {
                    config = Some(PathBuf::from(
                        args.next()
                            .ok_or("--ssh-config needs a trusted absolute config path")?,
                    ))
                }
                Some("--ssh-port") => {
                    port = args
                        .next()
                        .ok_or("--ssh-port needs a port")?
                        .to_string_lossy()
                        .parse()
                        .map_err(|_| "Invalid SSH port")?
                }
                Some("--ssh-takeover") => takeover = true,
                Some("--session-dir") => {
                    options.session_dir =
                        Some(args.next().ok_or("--session-dir needs a directory")?.into())
                }
                Some("--new") => options.new_buffer = true,
                Some("--wait" | "-w") => options.wait = true,
                Some("--help" | "-h") => options.help = true,
                Some("--version" | "-V") => options.version = true,
                Some("-") => options.stdin = true,
                Some(s) if s.starts_with('-') => {
                    return Err(format!(
                        "Unknown option: {s}. Use -- before a filename beginning with '-'."
                    ))
                }
                _ => options.files.push(arg.into()),
            }
        }
        if let Some(host) = ssh {
            options.remote = Some(crate::remote::Config {
                target: hedit_remote::client::SshTarget {
                    host,
                    user,
                    port,
                    config,
                    helper: helper.ok_or("--ssh-helper is required")?,
                    state: state.ok_or("--ssh-state is required")?,
                },
                root: root.ok_or("--ssh-root is required")?,
                takeover,
            });
            if options.stdin || options.wait {
                return Err("SSH windows do not accept local stdin or --wait.".into());
            }
        } else if root.is_some()
            || helper.is_some()
            || state.is_some()
            || user.is_some()
            || config.is_some()
            || port != 22
            || takeover
        {
            return Err("SSH options require --ssh HOST.".into());
        }
        Ok(options)
    }
}

pub fn location(path: &Path) -> (PathBuf, u64, u64) {
    if path.exists() {
        return (path.to_owned(), 1, 1);
    }
    let Some(s) = path.to_str() else {
        return (path.to_owned(), 1, 1);
    };
    let Some((prefix, last)) = s.rsplit_once(':') else {
        return (path.to_owned(), 1, 1);
    };
    let Some(last) = last.parse::<u64>().ok().filter(|n| *n > 0) else {
        return (path.to_owned(), 1, 1);
    };
    if let Some((file, line)) = prefix.rsplit_once(':') {
        if let Some(line) = line.parse::<u64>().ok().filter(|n| *n > 0) {
            return (PathBuf::from(file), line, last);
        }
    }
    (PathBuf::from(prefix), last, 1)
}
