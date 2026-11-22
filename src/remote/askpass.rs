//! Local GUI askpass invocation. The credential reply travels through the
//! private stdout pipe created by OpenSSH.
pub use hedit_remote::client::AUTH_TIMEOUT;
use std::{ffi::OsStr, process::Command, time::Duration};

pub const REQUEST: &str = "HEDIT_SSH_ASKPASS";
pub const HOST: &str = "HEDIT_SSH_ASKPASS_HOST";
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(90);
pub const SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "macos"));

pub(crate) fn configure(command: &mut Command, host: &str) -> Result<(), String> {
    if !SUPPORTED {
        return Err("Interactive SSH authentication is available on Linux and macOS.".into());
    }
    if cfg!(target_os = "linux")
        && ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .all(|key| std::env::var_os(key).is_none_or(|value| value.is_empty()))
    {
        return Err("Interactive SSH authentication requires a graphical session.".into());
    }
    let executable = crate::windows::executable().map_err(|error| error.to_string())?;
    command
        .env("SSH_ASKPASS", executable)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env(REQUEST, "1")
        .env(HOST, host);
    // The SSH command otherwise receives only its normal scrubbed environment.
    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "XAUTHORITY",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    Ok(())
}

pub(crate) fn setup_timeout(command: &Command) -> Duration {
    if command
        .get_envs()
        .any(|(key, value)| key == OsStr::new(REQUEST) && value == Some(OsStr::new("1")))
    {
        AUTH_TIMEOUT
    } else {
        Duration::from_secs(60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_setup_has_its_own_timeout() {
        let mut command = Command::new("unused");
        assert_eq!(setup_timeout(&command), Duration::from_secs(60));
        command.env(REQUEST, "1");
        assert_eq!(setup_timeout(&command), AUTH_TIMEOUT);
    }
}
