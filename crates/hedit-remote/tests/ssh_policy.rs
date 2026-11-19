use hedit_remote::client::SshTarget;

fn target() -> SshTarget {
    SshTarget {
        routing: Default::default(),
        host: "dev-box".into(),
        user: Some("alice".into()),
        port: 22,
        config: None,
        helper: "/opt/hedit/helper".into(),
        state: "/home/alice/.state/remote".into(),
    }
}

#[test]
fn dangerous_destination_forms_cannot_become_ssh_options_or_shell_code() {
    for host in [
        "-oProxyCommand=bad",
        "host;touch /tmp/x",
        "host\nother",
        "a@b",
        "$(cmd)",
        "`cmd`",
        "",
    ] {
        let mut t = target();
        t.host = host.into();
        assert!(t.command().is_err(), "{host:?}");
    }
    for user in ["-o", "a b", "a;cmd", "a\n", ""] {
        let mut t = target();
        t.user = Some(user.into());
        assert!(t.command().is_err(), "{user:?}");
    }
}

#[test]
fn arguments_override_unsafe_forwarding_and_connection_reuse() {
    let command = target().command().unwrap();
    let args: Vec<_> = command
        .get_args()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    for option in [
        "StrictHostKeyChecking=yes",
        "ForwardAgent=no",
        "ForwardX11=no",
        "ClearAllForwardings=yes",
        "PermitLocalCommand=no",
        "ControlPath=none",
        "Tunnel=no",
        "BatchMode=yes",
        "UpdateHostKeys=no",
        "ProxyJump=none",
        "ProxyCommand=none",
    ] {
        assert!(args.windows(2).any(|w| w == ["-o", option]), "{option}");
    }
    assert!(args.windows(2).any(|w| w == ["--", "dev-box"]));
}

#[test]
fn interactive_commands_keep_strict_ssh_policy() {
    let target = target();
    let command = target.command_with_auth(true).unwrap();
    let args: Vec<_> = command.get_args().map(|s| s.to_str().unwrap()).collect();
    assert!(!args.contains(&"BatchMode=yes"));
    for option in [
        "BatchMode=no",
        "NumberOfPasswordPrompts=1",
        "StrictHostKeyChecking=yes",
        "ForwardAgent=no",
        "ControlMaster=no",
        "PermitLocalCommand=no",
        "UpdateHostKeys=no",
    ] {
        assert!(
            args.windows(2).any(|pair| pair == ["-o", option]),
            "{option}"
        );
    }
}

#[test]
fn trusted_install_paths_are_shell_quoted_and_invalid_ones_rejected() {
    let mut t = target();
    t.helper = "/opt/a ' quote/helper".into();
    let cmd = t.command().unwrap();
    let last = cmd.get_args().last().unwrap().to_str().unwrap();
    assert!(last.starts_with("exec '/opt/a '\\'' quote/helper' bridge --state '"));
    for path in ["relative/helper", "/opt/bad\nhelper", "/opt/\0helper"] {
        t.helper = path.into();
        assert!(t.command().is_err());
    }
}

#[test]
fn config_ports_are_inherited_unless_explicitly_overridden() {
    let mut t = target();
    t.port = 0;
    let command = t.command().unwrap();
    assert!(!command.get_args().any(|a| a == "-p"));
    t.port = 2222;
    let command = t.command().unwrap();
    let args = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(args.windows(2).any(|w| w == ["-p", "2222"]));
}

#[cfg(unix)]
#[test]
fn openssh_resolves_alias_user_port_and_identity_from_selected_config() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    std::fs::write(&config, "Host dev-box\n HostName 127.0.0.1\n User configured-user\n Port 2244\n IdentityFile /keys/dev\n").unwrap();
    let mut t = target();
    t.port = 0;
    t.user = None;
    t.config = Some(config);
    let base = t.command().unwrap();
    let mut command = std::process::Command::new(base.get_program());
    command.arg("-G").args(base.get_args());
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = String::from_utf8(output.stdout).unwrap();
    for line in [
        "hostname 127.0.0.1",
        "user configured-user",
        "port 2244",
        "identityfile /keys/dev",
    ] {
        assert!(config.lines().any(|s| s == line), "missing {line}");
    }
}

#[cfg(unix)]
#[test]
fn opted_in_proxy_jump_uses_verified_openssh_hops_and_keeps_ambient_forwards_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("test config % ' example");
    std::fs::write(
        &config,
        "Host dev-box\n HostName worker.example\n Port 2299\n ProxyJump dev@first:2224,second\n LocalForward 18000 localhost:8000\nHost second\n HostName second.example\n Port 2225\n",
    ).unwrap();
    let mut t = target();
    t.port = 0;
    t.config = Some(config);
    let direct = t.command().unwrap();
    assert!(direct.get_args().any(|arg| arg == "ProxyCommand=none"));
    t.routing.jump = true;
    t.routing.forwards = true;
    let routed = t.command().unwrap();
    assert!(routed
        .get_args()
        .any(|arg| arg == "ClearAllForwardings=yes"));
    assert!(routed.get_args().any(|arg| arg == "ControlMaster=no"));
    let proxy = routed
        .get_args()
        .filter_map(|arg| arg.to_str()?.strip_prefix("ProxyCommand="))
        .next()
        .unwrap();
    for expected in [
        "StrictHostKeyChecking=yes",
        "ForwardAgent=no",
        "ControlPath=none",
        "worker.example]:2299",
        "second.example]:2225",
        "2224",
        "first",
        "test config %%",
    ] {
        assert!(proxy.contains(expected), "missing {expected}");
    }
    let output = std::process::Command::new(routed.get_program())
        .arg("-G")
        .args(routed.get_args())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let settings = String::from_utf8(output.stdout).unwrap();
    assert!(settings
        .lines()
        .any(|line| line.starts_with("proxycommand ") && line.contains("[worker.example]:2299")));
    assert!(!settings
        .lines()
        .any(|line| line.starts_with("localforward ")));
}

#[cfg(unix)]
#[test]
fn jump_opt_in_does_not_inherit_proxy_commands_or_run_config_during_validation() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    std::fs::write(&config, "Host dev-box\n ProxyCommand something-custom\n").unwrap();
    let mut t = target();
    t.routing.jump = true;
    t.config = Some(config.clone());
    let result = t.command_for_detailed(":", false).unwrap_err();
    assert!(result.contains("Use ProxyJump"));
    std::fs::write(config, "Host dev-box\n UnsupportedKeyword test\n").unwrap();
    assert!(t.validate_settings(true).is_ok());
}
