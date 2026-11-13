//! Explicit OpenSSH arguments and a memory-only client with an RPC deadline.
use crate::protocol::{self, Envelope, Error, Reply, Request, Response, Result, VERSION};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, SyncSender},
    thread,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct SshTarget {
    pub host: String,
    pub user: Option<String>,
    pub port: u16,
    /// Explicitly trusted local configuration, never read from a workspace.
    pub config: Option<PathBuf>,
    /// Manually installed trusted executable and private state on the host.
    pub helper: String,
    pub state: String,
}

fn quote(s: &str) -> Result<String> {
    if s.contains(['\0', '\n', '\r']) || !s.starts_with('/') {
        return Err(Error::Permission);
    }
    Ok(format!("'{}'", s.replace('\'', "'\\''")))
}

impl SshTarget {
    pub fn command(&self) -> Result<Command> {
        if self.port == 0
            || self.host.is_empty()
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
        // Batch authentication only in this prototype; no ad hoc password UI.
        for option in [
            "BatchMode=yes",
            "StrictHostKeyChecking=yes",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ForwardX11Trusted=no",
            "GSSAPIDelegateCredentials=no",
            "ClearAllForwardings=yes",
            "PermitLocalCommand=no",
            "ControlMaster=no",
            "ControlPath=none",
            "ControlPersist=no",
            "RemoteCommand=none",
            "RequestTTY=no",
            "Tunnel=no",
            "EscapeChar=none",
            "ServerAliveInterval=5",
            "ServerAliveCountMax=3",
            "ConnectTimeout=10",
            "ConnectionAttempts=1",
            "UpdateHostKeys=no",
            // Hop-by-hop policy is not implemented by this direct-SSH prototype.
            "ProxyJump=none",
            "ProxyCommand=none",
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
        command.arg("-T").arg("-p").arg(self.port.to_string());
        if let Some(user) = &self.user {
            command.arg("-l").arg(user);
        }
        command.arg("--").arg(&self.host).arg(format!(
            "exec {} bridge --state {}",
            quote(&self.helper)?,
            quote(&self.state)?
        ));
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
}

struct Job {
    envelopes: Vec<Envelope>,
    reply: SyncSender<Result<Vec<Response>>>,
}
pub struct Client {
    child: Child,
    tx: Option<SyncSender<Job>>,
    worker: Option<thread::JoinHandle<()>>,
    next: u64,
    deadline: Duration,
    failed: bool,
}
impl Client {
    pub fn connect(target: &SshTarget) -> Result<Self> {
        Self::spawn(target.command()?)
    }

    pub fn spawn(mut command: Command) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        let mut input = child.stdin.take().ok_or(Error::Unavailable)?;
        let mut output = child.stdout.take().ok_or(Error::Unavailable)?;
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
        let mut client = Self {
            child,
            tx: Some(tx),
            worker: Some(worker),
            next: 0,
            deadline: Duration::from_secs(15),
            failed: false,
        };
        if !matches!(
            client.call(Request::Hello {})?,
            Response::Hello {
                version: VERSION,
                max_frame: protocol::MAX_FRAME,
                ..
            }
        ) {
            return Err(Error::Protocol);
        }
        Ok(client)
    }

    pub fn disconnect(&mut self) {
        self.failed = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
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

    fn fake(mode: &str) -> Client {
        let program = r#"
import sys,json,struct,time
def request():
    h=sys.stdin.buffer.read(4)
    n=struct.unpack('>I',h)[0]
    return json.loads(sys.stdin.buffer.read(n))
def reply(i,body):
    b=json.dumps(dict(version=1,id=i,result=dict(Ok=body))).encode()
    sys.stdout.buffer.write(struct.pack('>I',len(b))+b);sys.stdout.buffer.flush()
r=request();reply(r['id'],dict(kind='hello',version=1,max_frame=65536,session='a'*64,worker_pid=1))
r=request()
if sys.argv[1]=='wrong_id':reply(r['id']+1,dict(kind='pong'))
time.sleep(10)
"#;
        let mut command = Command::new("python3");
        command.arg("-c").arg(program).arg(mode);
        let mut client = Client::spawn(command).unwrap();
        client.deadline = Duration::from_millis(100);
        client
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
