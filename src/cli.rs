//! Small, native command line. No shell is used to open input.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanguageServerCommand {
    List,
    Install(String),
    Remove(String),
}

#[derive(Debug, Default)]
pub struct Options {
    pub language_server: Option<LanguageServerCommand>,
    pub language_server_directory: Option<PathBuf>,
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
        let mut port = 0;
        let mut interactive_auth = false;
        let mut takeover = false;
        while let Some(arg) = args.next() {
            if literal {
                options.files.push(arg.into());
                continue;
            }
            match arg.to_str() {
                Some("--") => literal = true,
                Some("--lsp") => {
                    if options.language_server.is_some() {
                        return Err("Specify --lsp only once.".into());
                    }
                    let action = args.next().ok_or("--lsp needs list, install, or remove")?;
                    options.language_server = Some(match action.to_str() {
                        Some("list") => LanguageServerCommand::List,
                        Some("install" | "remove") => {
                            let id = args
                                .next()
                                .and_then(|s| s.into_string().ok())
                                .ok_or("--lsp install/remove needs a server ID")?;
                            crate::language_server::manager::Manager::validate_id(&id)?;
                            if action == "install" {
                                LanguageServerCommand::Install(id)
                            } else {
                                LanguageServerCommand::Remove(id)
                            }
                        }
                        _ => return Err("--lsp needs list, install, or remove.".into()),
                    });
                }
                Some("--lsp-dir") => {
                    let path =
                        PathBuf::from(args.next().ok_or("--lsp-dir needs an absolute directory")?);
                    if !path.is_absolute() {
                        return Err("--lsp-dir must be absolute.".into());
                    }
                    options.language_server_directory = Some(path);
                }
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
                        .map_err(|_| "Invalid SSH port")?;
                    if port == 0 {
                        return Err("Invalid SSH port".into());
                    }
                }
                Some("--ssh-interactive") => interactive_auth = true,
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
            let managed = helper.is_none();
            if !managed && (helper.is_none() || state.is_none() || root.is_none()) {
                return Err(
                    "Custom SSH setup requires --ssh-root, --ssh-helper, and --ssh-state together."
                        .into(),
                );
            }
            options.remote = Some(crate::remote::Config {
                target: hedit_remote::client::SshTarget {
                    host,
                    user,
                    port,
                    config,
                    helper: helper.unwrap_or_default(),
                    state: state.unwrap_or_default(),
                },
                root: root.unwrap_or_default(),
                managed,
                interactive_auth,
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
            || port != 0
            || interactive_auth
            || takeover
        {
            return Err("SSH options require --ssh HOST.".into());
        }
        if options.language_server.is_some()
            && (!options.files.is_empty()
                || options.remote.is_some()
                || options.stdin
                || options.wait
                || options.new_buffer)
        {
            return Err(
                "--lsp cannot be combined with editor files, SSH, stdin, --wait, or --new.".into(),
            );
        }
        if options.language_server_directory.is_some() && options.language_server.is_none() {
            return Err("--lsp-dir requires --lsp.".into());
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

#[cfg(test)]
mod language_server_tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(OsString::from))
    }
    #[test]
    fn ssh_alias_only_uses_managed_setup_and_optional_workspace() {
        let config = parse(&["--ssh", "devbox"]).unwrap().remote.unwrap();
        assert!(config.managed && config.root.is_empty());
        assert_eq!(config.target.port, 0);
        assert!(config.target.user.is_none());
        assert!(parse(&["--ssh", "devbox", "--ssh-helper", "/helper"]).is_err());
        let config = parse(&["--ssh", "devbox", "--ssh-root", "/work"])
            .unwrap()
            .remote
            .unwrap();
        assert!(config.managed);
        assert_eq!(config.root, "/work");
        let config = parse(&["--ssh", "devbox", "--ssh-state", "/private/existing"])
            .unwrap()
            .remote
            .unwrap();
        assert!(config.managed);
        assert_eq!(config.target.state, "/private/existing");
    }
    #[test]
    fn ssh_port_inherits_config_but_explicit_zero_is_rejected() {
        let args = [
            "--ssh",
            "dev",
            "--ssh-root",
            "/work",
            "--ssh-helper",
            "/helper",
            "--ssh-state",
            "/state",
        ];
        assert_eq!(parse(&args).unwrap().remote.unwrap().target.port, 0);
        let mut explicit = args.to_vec();
        explicit.extend(["--ssh-port", "2222"]);
        assert_eq!(parse(&explicit).unwrap().remote.unwrap().target.port, 2222);
        *explicit.last_mut().unwrap() = "0";
        assert!(parse(&explicit).is_err());
    }

    #[test]
    fn ssh_interactive_authentication_is_opt_in() {
        assert!(
            !parse(&["--ssh", "dev"])
                .unwrap()
                .remote
                .unwrap()
                .interactive_auth
        );
        assert!(
            parse(&["--ssh", "dev", "--ssh-interactive"])
                .unwrap()
                .remote
                .unwrap()
                .interactive_auth
        );
        assert!(parse(&["--ssh-interactive"]).is_err());
    }

    #[test]
    fn language_server_commands_do_not_open_files_or_accept_ambiguous_arguments() {
        assert_eq!(
            parse(&["--lsp", "list"]).unwrap().language_server,
            Some(LanguageServerCommand::List)
        );
        assert_eq!(
            parse(&["--lsp", "install", "rust-analyzer"])
                .unwrap()
                .language_server,
            Some(LanguageServerCommand::Install("rust-analyzer".into()))
        );
        for args in [
            vec!["--lsp"],
            vec!["--lsp", "unknown"],
            vec!["--lsp", "install"],
            vec!["--lsp", "remove", "../escape"],
            vec!["--lsp", "list", "file.rs"],
            vec!["--lsp", "list", "--new"],
            vec!["--lsp-dir", "relative"],
            vec!["--lsp", "list", "--lsp", "list"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        assert_eq!(
            parse(&["--", "--lsp"]).unwrap().files,
            [PathBuf::from("--lsp")]
        );
    }
}
