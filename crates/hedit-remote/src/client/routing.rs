//! Resolve opted-in SSH routes, construct verified hops and own local forwards.
use super::SshTarget;
use std::{
    io::{self, Read},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const MAX_HOPS: usize = 4;
const MAX_FORWARDS: usize = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Routing {
    pub jump: bool,
    pub forwards: bool,
}

impl Routing {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Default)]
pub(super) struct Plan {
    pub proxy: Option<String>,
    forwards: Vec<LocalForward>,
}

impl Plan {
    pub(super) fn resolve(
        target: &SshTarget,
        interactive: bool,
        primary: bool,
    ) -> Result<Self, String> {
        let forwards = primary && target.routing.forwards;
        if !target.routing.jump && !forwards {
            return Ok(Self::default());
        }
        if !cfg!(unix) {
            return Err(
                "SSH config jump hosts and local forwards require a Linux or macOS client.".into(),
            );
        }
        let resolved = EffectiveConfig::query(target)?;
        let proxy = if target.routing.jump {
            proxy(target, &resolved, &resolved.jumps, interactive, MAX_HOPS)?
        } else {
            None
        };
        let forwards = if forwards {
            resolved
                .forwards
                .iter()
                .map(|rule| LocalForward::parse(rule))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        Ok(Self { proxy, forwards })
    }

    pub(super) fn prepare_forwards(
        self,
        command: &mut Command,
    ) -> Result<Option<Forwards>, String> {
        if self.forwards.is_empty() {
            return Ok(None);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            use std::path::Path;

            // Keep the per-connection socket short enough for Unix sockaddr_un.
            let directory = tempfile::Builder::new()
                .prefix("hedit-ssh-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir_in(Path::new("/tmp"))
                .map_err(|error| {
                    format!("Cannot create a private SSH forwarding socket directory: {error}")
                })?;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .map_err(|error| {
                    format!("Cannot protect the SSH forwarding socket directory: {error}")
                })?;
            let socket = directory.path().join("control");
            let mut control = Command::new(command.get_program());
            control
                .env_clear()
                .env("LC_ALL", "C")
                .args(["-F", "none", "-S"])
                .arg(&socket);
            command.args(["-o", "ControlMaster=yes", "-S"]).arg(socket);
            Ok(Some(Forwards {
                _directory: directory,
                control,
                rules: self.forwards,
            }))
        }
        #[cfg(not(unix))]
        Err("SSH config local forwards require a Linux or macOS client.".into())
    }
}

pub(super) struct Forwards {
    #[cfg(unix)]
    _directory: tempfile::TempDir,
    control: Command,
    rules: Vec<LocalForward>,
}

impl Forwards {
    pub(super) fn ports(&self) -> impl Iterator<Item = u16> + '_ {
        self.rules.iter().map(|rule| rule.local_port)
    }

    pub(super) fn activate(&mut self, alias: &str) -> Result<(), String> {
        self.control
            .args(["-o", "ExitOnForwardFailure=yes", "-O", "forward"]);
        for rule in &self.rules {
            self.control.arg("-L").arg(&rule.spec);
        }
        self.control.arg("--").arg(alias);
        if run(&mut self.control).is_err() {
            let ports = self
                .rules
                .iter()
                .map(|rule| rule.local_port.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "Cannot forward configured localhost ports ({ports}). Check for ports already in use and whether the SSH host allows forwarding."
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct EffectiveConfig {
    hostname: String,
    port: u16,
    jumps: Vec<SshTarget>,
    proxy_command: bool,
    forwards: Vec<String>,
}

impl EffectiveConfig {
    fn query(target: &SshTarget) -> Result<Self, String> {
        // This runs only after the user opts in on a trusted local SSH profile.
        // OpenSSH can evaluate Match exec when resolving a connection.
        let mut command = target
            .base_command(false)
            .map_err(|error| error.to_string())?;
        command.arg("-G");
        target.destination(&mut command);
        let output = run(&mut command)
            .map_err(|_| "Cannot resolve the selected SSH config for this host.")?;
        let output = std::str::from_utf8(&output)
            .map_err(|_| "OpenSSH returned an unreadable configuration.")?;
        let mut config = Self::default();
        for line in output.lines() {
            let Some((key, value)) = line.split_once(' ') else {
                continue;
            };
            match key {
                "hostname" => config.hostname = value.to_owned(),
                "port" => {
                    config.port =
                        port(value).ok_or("OpenSSH returned an invalid destination port.")?
                }
                "proxyjump" if value != "none" => {
                    if target.routing.jump {
                        for hop in value.split(',').take(MAX_HOPS + 1) {
                            if config.jumps.len() == MAX_HOPS {
                                return Err(
                                    "SSH config supports at most four jump hosts in hedit.".into(),
                                );
                            }
                            config.jumps.push(parse_jump(hop, target)?);
                        }
                    }
                }
                "proxycommand" if value != "none" => config.proxy_command = true,
                "localforward" if target.routing.forwards => {
                    if config.forwards.len() == MAX_FORWARDS {
                        return Err(
                            "SSH config supports at most 16 local forwards in hedit.".into()
                        );
                    }
                    config.forwards.push(value.to_owned());
                }
                _ => {}
            }
        }
        Ok(config)
    }

    fn address(&self) -> Result<String, String> {
        if self.port == 0 || !valid_host(&self.hostname) {
            return Err("OpenSSH returned an unsupported jump destination address.".into());
        }
        Ok(format!(
            "[{}]:{}",
            self.hostname.trim_matches(['[', ']']),
            self.port
        ))
    }
}

fn proxy(
    target: &SshTarget,
    resolved: &EffectiveConfig,
    hops: &[SshTarget],
    interactive: bool,
    remaining: usize,
) -> Result<Option<String>, String> {
    let Some((hop, earlier)) = hops.split_last() else {
        return if resolved.proxy_command {
            Err(
                "This SSH config selects ProxyCommand. Use ProxyJump for a hedit SSH profile."
                    .into(),
            )
        } else {
            Ok(None)
        };
    };
    if remaining == 0
        || hops
            .iter()
            .any(|hop| hop.host.eq_ignore_ascii_case(&target.host))
    {
        return Err("SSH jump route repeats a host or exceeds four hops.".into());
    }
    let hop_config = EffectiveConfig::query(hop)?;
    let path = if earlier.is_empty() {
        &hop_config.jumps
    } else {
        earlier
    };
    let hop_proxy = proxy(hop, &hop_config, path, interactive, remaining - 1)?;
    let mut command = hop
        .base_command(interactive)
        .map_err(|error| error.to_string())?;
    hop.set_proxy(&mut command, hop_proxy.as_deref());
    command.args([
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
    ]);
    command.arg("-W").arg(resolved.address()?);
    hop.destination(&mut command);
    // OpenSSH passes ProxyCommand to the local shell. Quote every argument and
    // escape percent tokens so resolved hostnames and config paths stay literal.
    let command = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|arg| {
            let arg = arg
                .to_str()
                .filter(|s| !s.contains(['\0', '\r', '\n']))
                .ok_or("SSH jump routes require single-line Unicode config paths.")?;
            let mut escaped = String::from("\"");
            for ch in arg.chars() {
                match ch {
                    '\\' | '"' | '$' | '`' => escaped.push('\\'),
                    '%' => escaped.push('%'),
                    _ => {}
                }
                escaped.push(ch);
            }
            escaped.push('"');
            Ok(escaped)
        })
        .collect::<Result<Vec<_>, &str>>()?;
    Ok(Some(command.join(" ")))
}

fn valid_host(host: &str) -> bool {
    if host.contains(':') {
        return host
            .trim_matches(['[', ']'])
            .parse::<std::net::Ipv6Addr>()
            .is_ok();
    }
    crate::ssh_config::valid_alias(host)
}

fn port(value: &str) -> Option<u16> {
    value.parse().ok().filter(|port| *port != 0)
}

fn address(value: &str) -> Option<(&str, u16)> {
    let (host, number) = value.rsplit_once(':')?;
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    Some((host, port(number)?))
}

fn parse_jump(value: &str, target: &SshTarget) -> Result<SshTarget, String> {
    let value = value.strip_prefix("ssh://").unwrap_or(value);
    let (user, destination) = value
        .split_once('@')
        .map_or((None, value), |(user, host)| (Some(user.to_owned()), host));
    let (host, port) = if destination.starts_with('[') && destination.ends_with(']') {
        (&destination[1..destination.len() - 1], 0)
    } else if destination.parse::<std::net::Ipv6Addr>().is_ok() {
        (destination, 0)
    } else {
        address(destination).unwrap_or((destination, 0))
    };
    if !valid_host(host) {
        return Err("Unsupported SSH jump destination. Use an SSH alias or a host with an optional user and port.".into());
    }
    Ok(SshTarget {
        host: host.into(),
        user,
        port,
        config: target.config.clone(),
        helper: String::new(),
        state: String::new(),
        routing: Routing {
            jump: true,
            forwards: false,
        },
    })
}

struct LocalForward {
    spec: String,
    local_port: u16,
}

impl LocalForward {
    fn parse(rule: &str) -> Result<Self, String> {
        let invalid = || {
            "Use TCP LocalForward rules with a localhost bind and an explicit destination port."
                .to_owned()
        };
        let mut fields = rule.split_ascii_whitespace();
        let bind = fields.next().ok_or_else(invalid)?;
        let destination = fields.next().ok_or_else(invalid)?;
        let (local, local_port) = port(bind)
            .map(|port| ("localhost", port))
            .or_else(|| address(bind))
            .ok_or_else(invalid)?;
        let local = if local.eq_ignore_ascii_case("localhost") {
            "localhost"
        } else {
            local
        };
        let (remote, remote_port) = address(destination).ok_or_else(invalid)?;
        let loopback = local == "localhost"
            || local
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if !loopback || !valid_host(remote) || fields.next().is_some() {
            return Err(invalid());
        }
        Ok(Self {
            spec: format!("[{local}]:{local_port}:[{remote}]:{remote_port}"),
            local_port,
        })
    }
}

fn run(command: &mut Command) -> io::Result<Vec<u8>> {
    const LIMIT: u64 = 256 * 1024;
    const TIMEOUT: Duration = Duration::from_secs(10);
    let start = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped SSH output");
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout.take(LIMIT + 1).read_to_end(&mut output);
        let _ = tx.send(result.map(|_| output));
    });
    while start.elapsed() < TIMEOUT {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let output = rx
                    .recv_timeout(TIMEOUT.saturating_sub(start.elapsed()))
                    .map_err(io::Error::other)??;
                if output.len() as u64 > LIMIT {
                    return Err(io::Error::other(
                        "SSH configuration output exceeds its limit",
                    ));
                }
                return Ok(output);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Ok(Some(_)) => return Err(io::Error::other("OpenSSH command failed")),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "OpenSSH command timed out",
    ))
}

#[cfg(test)]
mod tests {
    use super::LocalForward;

    #[test]
    fn only_explicit_loopback_tcp_forward_rules_can_be_enabled() {
        for (rule, spec) in [
            (
                "8080 [service.internal]:80",
                "[localhost]:8080:[service.internal]:80",
            ),
            ("[127.0.0.2]:8080 [::1]:80", "[127.0.0.2]:8080:[::1]:80"),
            ("[::1]:1111 [localhost]:2222", "[::1]:1111:[localhost]:2222"),
        ] {
            assert_eq!(LocalForward::parse(rule).unwrap().spec, spec);
        }
        for rule in [
            "[*]:8080 [localhost]:80",
            "[0.0.0.0]:8080 [localhost]:80",
            "[::]:8080 [localhost]:80",
            "[another-host]:8080 [localhost]:80",
            "8080 /tmp/service.sock",
            "8080 [$(command)]:80",
            "8080 [localhost]:0",
            "8080 [localhost]:80 extra",
        ] {
            assert!(LocalForward::parse(rule).is_err(), "{rule}");
        }
    }
}
