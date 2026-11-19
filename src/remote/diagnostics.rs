//! Bounded connection events contain fixed text and counters, never SSH output.
use std::{collections::VecDeque, fmt, fmt::Write};

const LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SetupPhase {
    #[default]
    Connect,
    CheckHelper,
    DownloadHelper,
    InstallHelper,
    OpenWorkspace,
}

impl SetupPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connect => "SSH connection and authentication",
            Self::CheckHelper => "Installed helper check",
            Self::DownloadHelper => "Matching helper download",
            Self::InstallHelper => "Helper installation",
            Self::OpenWorkspace => "Remote workspace selection",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    HostKey,
    Authentication,
    Connect,
    Settings,
    Network,
    Forwarding,
    Timeout,
    RecoverySession,
    Writer,
    Reconciliation,
    Helper,
    Storage,
    Unknown,
}

impl Failure {
    pub fn classify(error: &str) -> Self {
        let error = error.to_ascii_lowercase();
        let has = |patterns: &[&str]| patterns.iter().any(|pattern| error.contains(pattern));
        if has(&["recovery session changed"]) {
            Self::RecoverySession
        } else if has(&[
            "cannot acquire ssh workspace writer",
            "cannot acquire workspace writer",
        ]) {
            Self::Writer
        } else if has(&[
            "host key verification failed",
            "remote host identification has changed",
        ]) {
            Self::HostKey
        } else if has(&[
            "permission denied (publickey",
            "permission denied (keyboard-interactive",
            "permission denied (password",
            "too many authentication failures",
        ]) {
            Self::Authentication
        } else if has(&[
            "bad configuration option",
            "bad owner or permissions on",
            "invalid ssh settings",
            "could not open user configuration file",
            "bad port",
        ]) {
            Self::Settings
        } else if has(&["timed out", "timeout"]) {
            Self::Timeout
        } else if has(&[
            "stdio forwarding failed",
            "administratively prohibited",
            "channel_connect_stdio_fwd",
        ]) {
            Self::Forwarding
        } else if has(&[
            "connection refused",
            "could not resolve hostname",
            "network is unreachable",
            "no route to host",
            "connection reset",
            "broken pipe",
        ]) {
            Self::Network
        } else if has(&["replay", "reconciliation", "durability receipt"]) {
            Self::Reconciliation
        } else if has(&[
            "unsafe directory permissions",
            "durable recovery storage",
            "disk quota",
            "no space left",
            "recovery directory",
        ]) {
            Self::Storage
        } else if has(&["cannot connect securely"]) {
            Self::Connect
        } else if has(&[
            "installed helper",
            "ssh helper",
            "cannot identify the ssh account",
            "unsupported ssh",
        ]) {
            Self::Helper
        } else {
            Self::Unknown
        }
    }

    pub fn advice(self) -> &'static str {
        match self {
            Self::HostKey => "Verify the host key in OpenSSH, then retry.",
            Self::Authentication => "Check the SSH user and load your key into the SSH agent, then retry.",
            Self::Connect => "Check the SSH configuration, pinned host key, SSH agent, and installed helper.",
            Self::Settings => "Review the SSH configuration and connection settings, then retry.",
            Self::Network => "Check the host address, SSH port, and network connection, then retry.",
            Self::Forwarding => "Check that the SSH host or jump host permits the requested forwarding.",
            Self::Timeout => "The operation timed out. Check that the host and SSH port are reachable, then retry.",
            Self::RecoverySession => "The host recovery session changed. Reconnect to the original recovery directory to resume pending edits.",
            Self::Writer => "Close the other writer or review the workspace takeover settings.",
            Self::Reconciliation => "Pending edits could not be reconciled. Review the workspace state before reconnecting.",
            Self::Helper => "Check the remote account and matching helper installation, then retry.",
            Self::Storage => "Check the permissions and free space of the private recovery directory.",
            Self::Unknown => "Check SSH connectivity, helper setup, and workspace permissions, then retry.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Setup(SetupPhase),
    SetupFailed(SetupPhase, Failure),
    SetupCancelled,
    SetupWorkerStopped,
    Launch,
    LaunchFailed(Failure),
    WindowReady,
    Connected,
    Disconnected,
    TransportLost(Failure),
    Reconnecting(Option<u8>),
    Reconnected,
    ReconnectFailed(Failure),
}

impl Event {
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            Self::SetupFailed(..)
                | Self::SetupWorkerStopped
                | Self::LaunchFailed(_)
                | Self::TransportLost(_)
                | Self::ReconnectFailed(_)
        )
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(phase) => f.write_str(phase.label()),
            Self::SetupFailed(phase, failure) => write!(
                f,
                "SSH setup failed during {}. {}",
                phase.label().to_lowercase(),
                failure.advice()
            ),
            Self::SetupCancelled => f.write_str("SSH setup cancelled."),
            Self::SetupWorkerStopped => {
                f.write_str("SSH setup worker stopped. Retry the connection.")
            }
            Self::Launch => f.write_str("Open the remote workspace window."),
            Self::LaunchFailed(failure) => write!(
                f,
                "Could not open the remote workspace window. {}",
                failure.advice()
            ),
            Self::WindowReady => f.write_str("Remote workspace window opened."),
            Self::Connected => f.write_str("Connected to the SSH workspace."),
            Self::Disconnected => f.write_str("Disconnected. Pending edits remain in the window."),
            Self::TransportLost(failure) => {
                write!(f, "Lost the SSH workspace connection. {}", failure.advice())
            }
            Self::Reconnecting(Some(attempt)) => {
                write!(f, "Automatic reconnect, attempt {attempt} of 5.")
            }
            Self::Reconnecting(None) => f.write_str("Reconnecting to the SSH workspace."),
            Self::Reconnected => f.write_str("Reconnected and reconciled pending edits."),
            Self::ReconnectFailed(failure) => {
                write!(f, "SSH reconnect failed. {}", failure.advice())
            }
        }
    }
}

#[derive(Default)]
pub struct Log {
    entries: VecDeque<Event>,
}

impl Log {
    pub fn record(&mut self, event: Event) {
        if self.entries.len() == LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back(event);
    }

    pub fn summary(&self) -> String {
        let mut summary = String::from("hedit SSH diagnostics\n");
        for event in &self.entries {
            let _ = writeln!(summary, "{event}");
        }
        summary
    }

    pub fn entries(&self) -> &VecDeque<Event> {
        &self.entries
    }

    pub(super) fn drain(&mut self) -> Vec<Event> {
        self.entries.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_retains_recent_events_and_excludes_raw_output() {
        let mut log = Log::default();
        log.record(Event::Connected);
        for _ in 0..LIMIT {
            log.record(Event::Reconnected);
        }
        let raw = "me@private.example: Permission denied (publickey). Secret=do-not-copy";
        let failure = Failure::classify(raw);
        assert_eq!(failure, Failure::Authentication);
        log.record(Event::SetupFailed(SetupPhase::Connect, failure));
        assert_eq!(log.entries.len(), LIMIT);
        assert_eq!(log.entries[0], Event::Reconnected);
        let summary = log.summary();
        assert!(summary.contains("SSH agent"));
        assert!(!summary.contains("private.example"));
        assert!(!summary.contains("do-not-copy"));
    }
}
