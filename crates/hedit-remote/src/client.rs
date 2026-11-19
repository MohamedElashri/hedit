//! Explicit OpenSSH arguments and a memory-only client with an RPC deadline.
use crate::protocol::{self, Envelope, Error, Reply, Request, Response, Result, VERSION};
mod routing;
pub use routing::Routing;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, SyncSender},
    thread,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SshTarget {
    pub host: String,
    pub user: Option<String>,
    /// Zero inherits Port from OpenSSH configuration (or OpenSSH's default).
    pub port: u16,
    /// Explicitly trusted local configuration, never read from a workspace.
    pub config: Option<PathBuf>,
    /// Opt in to approved routing options from the selected local SSH config.
    #[serde(default, skip_serializing_if = "Routing::is_default")]
    pub routing: Routing,
    /// Trusted executable and private state on the host.
    pub helper: String,
    pub state: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupInfo {
    pub version: String,
    pub uid: u32,
    pub home: String,
    pub root: String,
    pub helper: String,
    pub state: String,
}
pub const SETUP_VERSION: u32 = 1;

fn quote(s: &str) -> Result<String> {
    if s.contains(['\0', '\n', '\r']) || !s.starts_with('/') {
        return Err(Error::Permission);
    }
    Ok(format!("'{}'", s.replace('\'', "'\\''")))
}

impl SshTarget {
    pub fn validate_settings(&self, managed: bool) -> Result<()> {
        self.base_command(false)?;
        if !managed {
            self.bridge_command()?;
        }
        Ok(())
    }

    pub fn command(&self) -> Result<Command> {
        self.command_for(&self.bridge_command()?)
    }

    fn bridge_command(&self) -> Result<String> {
        Ok(format!(
            "exec {} bridge --state {}",
            quote(&self.helper)?,
            quote(&self.state)?
        ))
    }

    /// Run application-owned bootstrap code using exactly the RPC SSH policy.
    /// Callers must quote any data interpolated into the remote command.
    pub fn command_for(&self, remote_command: &str) -> Result<Command> {
        self.command_for_with_auth(remote_command, false)
    }

    pub fn command_for_with_auth(
        &self,
        remote_command: &str,
        interactive: bool,
    ) -> Result<Command> {
        self.command_for_detailed(remote_command, interactive)
            .map_err(|_| Error::Permission)
    }

    pub fn command_for_detailed(
        &self,
        remote_command: &str,
        interactive: bool,
    ) -> std::result::Result<Command, String> {
        let plan = routing::Plan::resolve(self, interactive, false)?;
        let mut command = self.routed_command(interactive, &plan)?;
        Self::disable_control(&mut command);
        self.destination(&mut command);
        command.arg(remote_command);
        Ok(command)
    }

    fn routed_command(
        &self,
        interactive: bool,
        plan: &routing::Plan,
    ) -> std::result::Result<Command, String> {
        let mut command = self
            .base_command(interactive)
            .map_err(|error| error.to_string())?;
        self.set_proxy(&mut command, plan.proxy.as_deref());
        command.args(["-o", "ClearAllForwardings=yes", "-T"]);
        Ok(command)
    }

    fn disable_control(command: &mut Command) {
        command.args(["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
    }

    fn set_proxy(&self, command: &mut Command, proxy: Option<&str>) {
        // An application-generated ssh -W command keeps the same verification
        // policy on every hop. The configured ProxyCommand is never inherited.
        command
            .arg("-o")
            .arg(format!("ProxyCommand={}", proxy.unwrap_or("none")));
        command.args(["-o", "ProxyJump=none", "-o", "ProxyUseFdpass=no"]);
    }

    fn base_command(&self, interactive: bool) -> Result<Command> {
        if self.host.is_empty()
            || self.host.starts_with('-')
            || self.host.len() > 253
            || !self
                .host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_:[]".contains(&b))
        {
            return Err(Error::Permission);
        }
        if self.user.as_ref().is_some_and(|u| {
            u.is_empty()
                || u.starts_with('-')
                || u.len() > 128
                || !u
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        }) {
            return Err(Error::Permission);
        }
        #[cfg(unix)]
        let exe = PathBuf::from("/usr/bin/ssh");
        #[cfg(windows)]
        let exe = PathBuf::from(std::env::var_os("SystemRoot").ok_or(Error::Unavailable)?)
            .join("System32/OpenSSH/ssh.exe");
        let mut command = Command::new(exe);
        #[cfg(unix)]
        command.current_dir("/");
        #[cfg(windows)]
        command.current_dir(std::env::temp_dir());
        command.args([
            "-o",
            if interactive {
                "BatchMode=no"
            } else {
                "BatchMode=yes"
            },
        ]);
        for option in [
            "StrictHostKeyChecking=yes",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ForwardX11Trusted=no",
            "GSSAPIDelegateCredentials=no",
            "PermitLocalCommand=no",
            "ControlPersist=no",
            "ForkAfterAuthentication=no",
            "StdinNull=no",
            "RemoteCommand=none",
            "RequestTTY=no",
            "Tunnel=no",
            "EscapeChar=none",
            "ServerAliveInterval=5",
            "ServerAliveCountMax=3",
            "ConnectTimeout=10",
            "ConnectionAttempts=1",
            "UpdateHostKeys=no",
            "GatewayPorts=no",
            "LogLevel=ERROR",
        ] {
            command.arg("-o").arg(option);
        }
        if let Some(config) = &self.config {
            if !config.is_absolute() {
                return Err(Error::Permission);
            }
            command.arg("-F").arg(config);
        }
        // Do not transmit editor/tool environment secrets through inherited SendEnv.
        let allowed = [
            "HOME",
            "USER",
            "LOGNAME",
            "SSH_AUTH_SOCK",
            "SystemRoot",
            "WINDIR",
            "PATH",
        ];
        let environment: Vec<_> = allowed
            .into_iter()
            .filter_map(|key| std::env::var_os(key).map(|v| (key, v)))
            .collect();
        command.env_clear().envs(environment).env("LC_ALL", "C");
        Ok(command)
    }

    fn destination(&self, command: &mut Command) {
        if self.port != 0 {
            command.arg("-p").arg(self.port.to_string());
        }
        if let Some(user) = &self.user {
            command.arg("-l").arg(user);
        }
        command.arg("--").arg(&self.host);
    }
}

struct Job {
    envelopes: Vec<Envelope>,
    reply: SyncSender<Result<Vec<Response>>>,
}
pub struct Client {
    forwards: Option<routing::Forwards>,
    child: Child,
    tx: Option<SyncSender<Job>>,
    worker: Option<thread::JoinHandle<()>>,
    next: u64,
    deadline: Duration,
    failed: bool,
    diagnostics: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}
impl Client {
    pub fn connect(target: &SshTarget) -> Result<Self> {
        Self::spawn(target.command()?)
    }

    pub fn connect_detailed(target: &SshTarget) -> std::result::Result<Self, String> {
        Self::connect_with_session(target).map(|(client, _)| client)
    }

    /// Connect and return the recovery session from the validated handshake.
    pub fn connect_with_session(target: &SshTarget) -> std::result::Result<(Self, String), String> {
        Self::connect_with_session_using(target, false, |_| Ok(()))
    }

    pub fn connect_with_session_using(
        target: &SshTarget,
        interactive: bool,
        configure: impl FnOnce(&mut Command) -> std::result::Result<(), String>,
    ) -> std::result::Result<(Self, String), String> {
        let bridge = target.bridge_command().map_err(|error| error.to_string())?;
        let plan = routing::Plan::resolve(target, interactive, true)?;
        let mut command = target.routed_command(interactive, &plan)?;
        let mut forwards = plan.prepare_forwards(&mut command)?;
        if forwards.is_none() {
            SshTarget::disable_control(&mut command);
        }
        target.destination(&mut command);
        command.arg(bridge);
        configure(&mut command)?;
        let (mut client, session) = Self::spawn_with_session(command)?;
        if let Some(forwards) = &mut forwards {
            forwards.activate(&target.host)?;
        }
        client.forwards = forwards;
        Ok((client, session))
    }

    fn spawn_with_session(command: Command) -> std::result::Result<(Self, String), String> {
        let mut client = Self::spawn_transport(command).map_err(|e| e.to_string())?;
        let session = client
            .handshake()
            .map_err(|e| format!("{e}: {}", client.diagnostics().trim()))?;
        Ok((client, session))
    }
    pub fn spawn(command: Command) -> Result<Self> {
        let mut client = Self::spawn_transport(command)?;
        client.handshake()?;
        Ok(client)
    }
    fn spawn_transport(mut command: Command) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        let mut input = child.stdin.take().ok_or(Error::Unavailable)?;
        let mut output = child.stdout.take().ok_or(Error::Unavailable)?;
        // Drain stderr for the lifetime of SSH, retaining only a bounded tail.
        // A full pipe must never stall the protocol or the child shutdown path.
        let diagnostics = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let tail = diagnostics.clone();
        let mut stderr = child.stderr.take().ok_or(Error::Unavailable)?;
        thread::spawn(move || {
            use std::io::Read;
            let mut chunk = [0; 1024];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut bytes = tail.lock().unwrap();
                bytes.extend_from_slice(&chunk[..n]);
                let remove = bytes.len().saturating_sub(8192);
                bytes.drain(..remove);
            }
        });
        let (tx, rx) = mpsc::sync_channel::<Job>(1);
        let worker = thread::spawn(move || {
            for job in rx {
                let result = (|| {
                    for envelope in &job.envelopes {
                        protocol::write_frame(&mut input, envelope).map_err(|_| Error::Protocol)?;
                    }
                    let mut replies = Vec::new();
                    let mut failure = None;
                    for envelope in &job.envelopes {
                        let reply: Reply = protocol::read_frame(&mut output)
                            .map_err(|_| Error::Protocol)?
                            .ok_or(Error::Unavailable)?;
                        if reply.version != VERSION || reply.id != envelope.id {
                            return Err(Error::Protocol);
                        }
                        match reply.result {
                            Ok(response) => replies.push(response),
                            Err(e) => failure = Some(e),
                        }
                    }
                    if let Some(error) = failure {
                        Err(error)
                    } else {
                        Ok(replies)
                    }
                })();
                let failed = matches!(result, Err(Error::Protocol | Error::Unavailable));
                let _ = job.reply.send(result);
                if failed {
                    break;
                }
            }
        });
        Ok(Self {
            forwards: None,
            child,
            tx: Some(tx),
            worker: Some(worker),
            next: 0,
            deadline: Duration::from_secs(15),
            failed: false,
            diagnostics,
        })
    }
    fn handshake(&mut self) -> Result<String> {
        let Response::Hello {
            session,
            version: VERSION,
            max_frame: protocol::MAX_FRAME,
            ..
        } = self.call(Request::Hello {})?
        else {
            return Err(Error::Protocol);
        };
        Ok(session)
    }

    pub fn disconnect(&mut self) {
        self.failed = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    pub fn is_failed(&self) -> bool {
        self.failed
    }
    pub fn forwarded_ports(&self) -> Vec<u16> {
        self.forwards
            .iter()
            .flat_map(routing::Forwards::ports)
            .collect()
    }
    /// Memory only; never write SSH diagnostics into document recovery.
    pub fn diagnostics(&self) -> String {
        String::from_utf8_lossy(&self.diagnostics.lock().unwrap())
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect()
    }
    pub fn call(&mut self, request: Request) -> Result<Response> {
        let response = if protocol::encode(&Envelope {
            version: VERSION,
            id: self.next,
            request: request.clone(),
        })
        .is_ok()
        {
            self.frame(request)?
        } else {
            let bytes =
                protocol::encode_limit(&request, protocol::MAX_VALUE).map_err(|_| Error::Limit)?;
            let Response::Upload { id } = self.frame(Request::UploadStart {
                length: bytes.len() as u64,
            })?
            else {
                return Err(Error::Protocol);
            };
            for window in bytes.chunks(40 * 1024).collect::<Vec<_>>().chunks(4) {
                self.frames(
                    window
                        .iter()
                        .map(|chunk| Request::UploadWrite {
                            id,
                            bytes: chunk.to_vec(),
                        })
                        .collect(),
                )?;
            }
            self.frame(Request::UploadRun { id })?
        };
        if let Response::Transfer { id, length } = response {
            let length = usize::try_from(length).map_err(|_| Error::Limit)?;
            if length > protocol::MAX_VALUE {
                return Err(Error::Limit);
            }
            let mut bytes = Vec::new();
            while bytes.len() < length {
                let start = bytes.len();
                let requests = (start..length)
                    .step_by(40 * 1024)
                    .take(4)
                    .map(|offset| Request::Download {
                        id,
                        offset: offset as u64,
                    })
                    .collect();
                for response in self.frames(requests)? {
                    let Response::Bytes { bytes: chunk, done } = response else {
                        return Err(Error::Protocol);
                    };
                    if chunk.is_empty() || bytes.len() + chunk.len() > length {
                        return Err(Error::Protocol);
                    }
                    bytes.extend_from_slice(&chunk);
                    if done && bytes.len() != length {
                        return Err(Error::Protocol);
                    }
                }
            }
            let result: Response = serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)?;
            if matches!(result, Response::Transfer { .. }) {
                return Err(Error::Protocol);
            }
            Ok(result)
        } else {
            Ok(response)
        }
    }
    fn frame(&mut self, request: Request) -> Result<Response> {
        self.frames(vec![request])?.pop().ok_or(Error::Protocol)
    }
    fn frames(&mut self, requests: Vec<Request>) -> Result<Vec<Response>> {
        if self.failed {
            return Err(Error::Unavailable);
        }
        if requests.is_empty() || requests.len() > 4 {
            return Err(Error::Limit);
        }
        let mut envelopes = Vec::new();
        for request in requests {
            let id = self.next;
            self.next = self.next.checked_add(1).ok_or(Error::Limit)?;
            let envelope = Envelope {
                version: VERSION,
                id,
                request,
            };
            protocol::encode(&envelope).map_err(|_| Error::Limit)?;
            envelopes.push(envelope);
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .as_ref()
            .ok_or(Error::Unavailable)?
            .try_send(Job {
                envelopes,
                reply: tx,
            })
            .map_err(|_| Error::Busy)?;
        match rx.recv_timeout(self.deadline) {
            Ok(result) => {
                if matches!(result, Err(Error::Protocol | Error::Unavailable)) {
                    self.failed = true;
                    let _ = self.child.kill();
                }
                result
            }
            Err(_) => {
                self.failed = true;
                let _ = self.child.kill();
                Err(Error::Unavailable)
            }
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.tx.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(worker) = self.worker.take() {
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn fake_command(mode: &str) -> Command {
        let program = r#"
import sys,json,struct,time
def request():
    h=sys.stdin.buffer.read(4)
    n=struct.unpack('>I',h)[0]
    return json.loads(sys.stdin.buffer.read(n))
def reply(i,body):
    b=json.dumps(dict(version=wire_version,id=i,result=dict(Ok=body))).encode()
    sys.stdout.buffer.write(struct.pack('>I',len(b))+b);sys.stdout.buffer.flush()
r=request();wire_version=r['version'];mode=sys.argv[1]
assert r['request']['op']=='hello'
reply(r['id'],dict(kind='hello',version=wire_version,max_frame=42 if mode=='wrong_frame' else 65536,session='a'*64,worker_pid=1))
if mode=='wrong_frame':sys.exit(0)
r=request()
if mode=='hello_once':
    assert r['id']==1 and r['request']['op']=='ping'
    reply(r['id'],dict(kind='pong'))
    sys.exit(0)
if mode=='wrong_id':reply(r['id']+1,dict(kind='pong'))
time.sleep(10)
"#;
        let mut command = Command::new("python3");
        command.arg("-c").arg(program).arg(mode);
        command
    }

    fn fake(mode: &str) -> Client {
        let mut client = Client::spawn(fake_command(mode)).unwrap();
        client.deadline = Duration::from_millis(100);
        client
    }

    #[test]
    fn validated_handshake_returns_the_session_without_a_second_hello() {
        let (mut client, session) = Client::spawn_with_session(fake_command("hello_once")).unwrap();
        assert_eq!(session, "a".repeat(64));
        assert!(matches!(
            client.call(Request::Ping {}),
            Ok(Response::Pong {})
        ));
        assert!(Client::spawn_with_session(fake_command("wrong_frame")).is_err());
    }

    #[test]
    fn silent_helper_times_out_and_failed_transport_cannot_be_reused() {
        let mut client = fake("silent");
        let start = std::time::Instant::now();
        assert!(matches!(
            client.call(Request::Ping {}),
            Err(Error::Unavailable)
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            client.call(Request::Ping {}),
            Err(Error::Unavailable)
        ));
    }

    #[test]
    fn mismatched_reply_cannot_complete_another_request() {
        let mut client = fake("wrong_id");
        assert!(matches!(
            client.call(Request::Ping {}),
            Err(Error::Protocol)
        ));
        assert!(matches!(
            client.call(Request::Ping {}),
            Err(Error::Unavailable)
        ));
    }
}
