use hedit_remote::client::SshTarget;

fn target() -> SshTarget {
    SshTarget {
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
