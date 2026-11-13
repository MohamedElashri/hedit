//! Memory-only SSH workspace client. Only the host stores document recovery.
use hedit_core::{
    backend::{
        self, Backend, Call, DocumentChange, PtyEvent, RemoteBuffer, RemoteBuild, RemotePty,
    },
    wire_path,
};
use hedit_remote::{
    client::{Client, SshTarget},
    protocol::{Action, DocumentInfo, Request, Response},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
const URI_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'.')
    .remove(b'-')
    .remove(b'_')
    .remove(b'~');
type Reply = mpsc::SyncSender<Result<Response, String>>;
enum Work {
    Call(Request, Reply),
    Edit(Request, String),
    Barrier(mpsc::SyncSender<Result<(), String>>),
    Reconnect(Reply),
}
#[derive(Clone, Debug)]
pub struct Config {
    pub target: SshTarget,
    pub root: String,
    pub takeover: bool,
}
struct State {
    epoch: u64,
    token: String,
    session: String,
    connected: bool,
    error: Option<String>,
    pending: Vec<(Request, String)>,
    pending_bytes: usize,
    oldest: Option<Instant>,
    infos: HashMap<String, DocumentInfo>,
    changed: HashMap<u64, bool>,
    trusted: bool,
}
#[derive(Clone)]
pub struct Connection {
    pub config: Config,
    prefix: String,
    state: Arc<Mutex<State>>,
    tx: mpsc::SyncSender<Work>,
}
fn rpc(client: &mut Client, request: Request) -> Result<Response, String> {
    client
        .call(request)
        .map_err(|e| format!("SSH workspace: {e}. Remote recovery remains on the host."))
}
fn establish(
    config: &Config,
    expected: Option<&str>,
) -> Result<(Client, String, u64, String), String> {
    let mut client=Client::connect(&config.target).map_err(|e|format!("Cannot connect securely: {e}. Check the pinned host key, SSH key, and installed helper."))?;
    let Response::Hello { session, .. } = rpc(&mut client, Request::Hello {})? else {
        return Err("Invalid helper handshake.".into());
    };
    if expected.is_some_and(|s| s != session) {
        return Err("The host recovery session changed. Pending edits remain in memory; reconnect to the original state directory.".into());
    }
    rpc(
        &mut client,
        Request::Workspace {
            path: config.root.as_bytes().to_vec(),
        },
    )?;
    let start = Instant::now();
    let owner = loop {
        match client.call(Request::Acquire {
            takeover: config.takeover,
        }) {
            Ok(value) => break value,
            Err(hedit_remote::protocol::Error::Busy)
                if start.elapsed() < Duration::from_secs(2) =>
            {
                std::thread::sleep(Duration::from_millis(30))
            }
            Err(e) => return Err(format!("Cannot acquire SSH workspace writer: {e}")),
        }
    };
    let Response::Owner { epoch, token } = owner else {
        return Err("Cannot acquire workspace writer.".into());
    };
    Ok((client, session, epoch, token))
}
impl Connection {
    pub fn connect(config: Config) -> Result<Arc<Self>, String> {
        if !config.root.starts_with('/') || config.root.contains('\0') {
            return Err("Use an absolute SSH workspace path.".into());
        }
        let (mut client, session, epoch, token) = establish(&config, None)?;
        let state = Arc::new(Mutex::new(State {
            epoch,
            token,
            session: session.clone(),
            connected: true,
            error: None,
            pending: vec![],
            pending_bytes: 0,
            oldest: None,
            infos: HashMap::new(),
            changed: HashMap::new(),
            trusted: false,
        }));
        let (tx, rx) = mpsc::sync_channel(128);
        let shared = state.clone();
        let settings = config.clone();
        std::thread::spawn(move || {
            let mut change_cursor = 0usize;
            loop {
                let work = match rx.recv_timeout(Duration::from_millis(750)) {
                    Ok(w) => w,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(_) => {
                        let connected = shared.lock().unwrap().connected;
                        if connected {
                            let ids = {
                                let s = shared.lock().unwrap();
                                let mut ids =
                                    s.infos.values().map(|i| i.document).collect::<Vec<_>>();
                                ids.sort_unstable();
                                ids.dedup();
                                let total = ids.len();
                                let selected = ids
                                    .into_iter()
                                    .cycle()
                                    .skip(if total == 0 { 0 } else { change_cursor % total })
                                    .take(total.min(8))
                                    .collect::<Vec<_>>();
                                change_cursor = change_cursor.wrapping_add(8);
                                selected
                            };
                            match rpc(&mut client, Request::Changes { documents: ids }) {
                                Ok(Response::Value { value }) => {
                                    if let Ok(values) =
                                        serde_json::from_value::<Vec<(u64, bool)>>(value)
                                    {
                                        let mut s = shared.lock().unwrap();
                                        for (id, changed) in values {
                                            s.changed.insert(id, changed);
                                        }
                                    }
                                }
                                other => {
                                    let mut s = shared.lock().unwrap();
                                    s.connected = false;
                                    s.trusted = false;
                                    s.error =
                                        Some(other.err().unwrap_or_else(|| {
                                            "Invalid host change response.".into()
                                        }));
                                }
                            }
                        }
                        continue;
                    }
                };
                match work {
                    Work::Call(request, reply) => {
                        let result = if shared.lock().unwrap().connected {
                            rpc(&mut client, request)
                        } else {
                            Err(
                            "The remote workspace is disconnected. Reconnect before continuing."
                                .into(),
                        )
                        };
                        let _ = reply.send(result);
                    }
                    Work::Edit(request, path) => {
                        if !shared.lock().unwrap().connected {
                            continue;
                        }
                        let result = rpc(&mut client, request);
                        let mut s = shared.lock().unwrap();
                        match result {
                            Ok(Response::Durable { info, .. }) => {
                                if !s.pending.is_empty() {
                                    s.pending.remove(0);
                                }
                                s.pending_bytes = s.pending.iter().map(|(r, _)| edit_cost(r)).sum();
                                if s.pending.is_empty() {
                                    s.oldest = None;
                                }
                                if let Some(current) = s.infos.get_mut(&path) {
                                    if info.revision >= current.revision {
                                        *current = info;
                                    }
                                }
                            }
                            other => {
                                s.connected = false;
                                s.trusted = false;
                                s.error = Some(
                                    other
                                        .err()
                                        .unwrap_or_else(|| "Invalid durability receipt.".into()),
                                );
                            }
                        }
                    }
                    Work::Barrier(reply) => {
                        let s = shared.lock().unwrap();
                        let result = if s.connected && s.pending.is_empty() {
                            Ok(())
                        } else {
                            Err(s.error.clone().unwrap_or_else(|| {
                                "Remote edits are awaiting a durable host receipt.".into()
                            }))
                        };
                        let _ = reply.send(result);
                    }
                    Work::Reconnect(reply) => {
                        let expected = shared.lock().unwrap().session.clone();
                        client.disconnect();
                        let result: Result<Response, String> = (|| {
                            let (new, _, epoch, token) = establish(&settings, Some(&expected))?;
                            client = new;
                            {
                                let mut s = shared.lock().unwrap();
                                s.epoch = epoch;
                                s.token = token;
                                s.trusted = false;
                            }
                            let pending = shared.lock().unwrap().pending.clone();
                            for (mut request, path) in pending {
                                if let Request::Apply { epoch: e, .. } = &mut request {
                                    *e = epoch;
                                }
                                let Response::Durable { info, .. } = rpc(&mut client, request)?
                                else {
                                    return Err("Cannot reconcile pending edits.".into());
                                };
                                let mut s = shared.lock().unwrap();
                                s.pending.remove(0);
                                if let Some(current) = s.infos.get_mut(&path) {
                                    if info.revision >= current.revision {
                                        *current = info;
                                    }
                                }
                            }
                            let mut s = shared.lock().unwrap();
                            s.connected = true;
                            s.error = None;
                            s.pending_bytes = 0;
                            s.oldest = None;
                            Ok(Response::Pong {})
                        })();
                        if let Err(e) = &result {
                            let mut s = shared.lock().unwrap();
                            s.connected = false;
                            s.error = Some(e.clone());
                        }
                        let _ = reply.send(result);
                    }
                }
            }
        });
        let connection = Arc::new(Self {
            config,
            prefix: format!("hedit-ssh://{session}/"),
            state,
            tx,
        });
        backend::register(connection.prefix.clone(), connection.clone());
        Ok(connection)
    }
    pub fn root(&self) -> PathBuf {
        PathBuf::from(&self.prefix)
    }
    pub fn uri(&self, relative: &[u8]) -> PathBuf {
        if relative.first() == Some(&0) {
            return PathBuf::from(format!(
                "{}~scratch/{}",
                self.prefix,
                String::from_utf8_lossy(&relative[1..])
            ));
        }
        let escaped = relative
            .split(|b| *b == b'/')
            .map(|part| percent_encoding::percent_encode(part, URI_SET).to_string())
            .collect::<Vec<_>>()
            .join("/");
        PathBuf::from(format!("{}{escaped}", self.prefix))
    }
    pub fn relative(&self, path: &Path) -> Result<Vec<u8>, String> {
        let value = path.to_string_lossy().replace('\\', "/");
        let tail = if value == self.prefix.trim_end_matches('/') {
            ""
        } else {
            value
                .strip_prefix(&self.prefix)
                .ok_or("The path belongs to another workspace.")?
        };
        if let Some(id) = tail.strip_prefix("~scratch/") {
            let mut bytes = vec![0];
            bytes.extend_from_slice(id.as_bytes());
            return Ok(bytes);
        }
        let decoded = percent_encoding::percent_decode_str(tail).collect::<Vec<_>>();
        if decoded.starts_with(b"/")
            || decoded.contains(&0)
            || decoded
                .split(|b| *b == b'/')
                .any(|p| p == b".." || p == b".")
        {
            return Err("Invalid remote path.".into());
        }
        Ok(decoded)
    }
    pub fn path(&self, text: &str) -> Result<PathBuf, String> {
        if text.starts_with("hedit-ssh:") {
            let path = PathBuf::from(text);
            self.relative(&path)?;
            Ok(path)
        } else {
            if text.starts_with('/')
                && text != self.config.root
                && !text.starts_with(&format!("{}/", self.config.root.trim_end_matches('/')))
            {
                return Err("The path is outside this SSH workspace.".into());
            }
            let bytes = text
                .strip_prefix(&format!("{}/", self.config.root.trim_end_matches('/')))
                .unwrap_or(text)
                .trim_start_matches('/')
                .as_bytes();
            let p = self.uri(bytes);
            self.relative(&p)?;
            Ok(p)
        }
    }
    fn call(&self, request: Request) -> Result<Response, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .try_send(Work::Call(request, tx))
            .map_err(|_| "Remote request queue is full.")?;
        rx.recv_timeout(Duration::from_secs(30))
            .map_err(|_| "Remote request timed out; its outcome may be uncertain.")?
    }
    pub fn flush(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .try_send(Work::Barrier(tx))
            .map_err(|_| "Remote request queue is full.")?;
        rx.recv_timeout(Duration::from_secs(30))
            .map_err(|_| "Host durability confirmation timed out.")?
    }
    pub fn reconnect(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .try_send(Work::Reconnect(tx))
            .map_err(|_| "Remote request queue is full.")?;
        rx.recv_timeout(Duration::from_secs(60))
            .map_err(|_| "Reconnect timed out.")??;
        Ok(())
    }
    pub fn trust(&self, allow: bool) -> Result<(), String> {
        self.flush()?;
        let epoch = self.state.lock().unwrap().epoch;
        self.call(Request::Trust { epoch, allow })?;
        self.state.lock().unwrap().trusted = allow;
        Ok(())
    }
    pub fn writable(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.connected
            && s.error.is_none()
            && s.pending.len() < 64
            && s.pending_bytes < 1024 * 1024
            && s.oldest
                .is_none_or(|t| t.elapsed() < Duration::from_secs(2))
    }
    pub fn status(&self) -> (bool, bool, usize, String) {
        let s = self.state.lock().unwrap();
        (
            s.connected,
            s.trusted,
            s.pending.len(),
            s.error.clone().unwrap_or_else(|| {
                if s.pending.is_empty() {
                    "Durable on SSH host".into()
                } else {
                    "Waiting for host durability".into()
                }
            }),
        )
    }
    fn service(&self) -> Result<Client, String> {
        self.flush()?;
        let (session, epoch, token) = {
            let s = self.state.lock().unwrap();
            (s.session.clone(), s.epoch, s.token.clone())
        };
        let mut client = Client::connect(&self.config.target).map_err(|e| e.to_string())?;
        rpc(
            &mut client,
            Request::Bind {
                session,
                epoch,
                token,
            },
        )?;
        Ok(client)
    }
    pub fn metadata(&self, value: Option<Value>) -> Result<Value, String> {
        self.flush()?;
        let epoch = self.state.lock().unwrap().epoch;
        let Response::Value { value } = self.call(Request::Metadata { epoch, value })? else {
            return Err("Invalid host view metadata.".into());
        };
        Ok(value)
    }
    pub fn list(&self) -> Result<Vec<DocumentInfo>, String> {
        self.flush()?;
        let Response::Documents { items } = self.call(Request::List {})? else {
            return Err("Invalid recovery list.".into());
        };
        Ok(items)
    }
    fn read(&self, info: DocumentInfo) -> Result<RemoteBuffer, String> {
        let Response::Buffer {
            info: current,
            text,
        } = self.call(Request::Content {
            document: info.document,
        })?
        else {
            return Err("Invalid remote document content.".into());
        };
        if current.revision != info.revision || text.len() > hedit_remote::protocol::MAX_DOCUMENT {
            return Err("Remote document changed during read.".into());
        }
        Ok(RemoteBuffer {
            revision: info.revision,
            text,
            dirty: info.dirty,
            encoding: info.encoding,
            bom: info.bom,
        })
    }
    fn hydrate(&self, id: u64, info: DocumentInfo) -> Result<crate::document::Document, String> {
        let path = self.uri(&info.path);
        let buffer = self.read(info.clone())?;
        self.state
            .lock()
            .unwrap()
            .infos
            .insert(path.to_string_lossy().into_owned(), info.clone());
        crate::document::Document::remote_memory(
            id,
            path,
            buffer.text,
            info.revision,
            buffer.dirty,
            buffer.encoding,
            buffer.bom,
        )
    }
    pub fn recover(
        &self,
        id: u64,
        info: DocumentInfo,
    ) -> Result<crate::document::Document, String> {
        self.hydrate(id, info)
    }
    pub fn open(&self, id: u64, path: &Path) -> Result<crate::document::Document, String> {
        self.flush()?;
        let epoch = self.state.lock().unwrap().epoch;
        let Response::Document { info } = self.call(Request::Open {
            epoch,
            path: self.relative(path)?,
        })?
        else {
            return Err("Invalid opened document.".into());
        };
        self.hydrate(id, info)
    }
    pub fn scratch(&self, id: u64) -> Result<crate::document::Document, String> {
        self.flush()?;
        let epoch = self.state.lock().unwrap().epoch;
        let Response::Document { info } = self.call(Request::Scratch { epoch })? else {
            return Err("Cannot create host scratchpad.".into());
        };
        self.hydrate(id, info)
    }
    pub fn identity(&self, path: &Path) -> Result<u64, String> {
        Ok(self.info(path)?.document)
    }
    fn info(&self, path: &Path) -> Result<DocumentInfo, String> {
        self.state
            .lock()
            .unwrap()
            .infos
            .get(&path.to_string_lossy().into_owned())
            .cloned()
            .ok_or_else(|| "Remote buffer is no longer registered.".into())
    }
    fn action(&self, path: &Path, action: Action, expected_revision: u64) -> Result<(), String> {
        let key = path.to_string_lossy().into_owned();
        let mut s = self.state.lock().unwrap();
        if !s.connected || s.error.is_some() {
            return Err("Remote connection paused. Reconnect to keep editing; pending text remains in memory.".into());
        }
        let cost = match &action {
            Action::Edit { text, .. } | Action::EditBytes { text, .. } => text.len() + 128,
            _ => 128,
        };
        if s.pending.len() >= 64
            || s.pending_bytes + cost > 1024 * 1024
            || s.oldest
                .is_some_and(|t| t.elapsed() > Duration::from_secs(2))
        {
            return Err(
                "Host durability is behind. Wait for receipts before editing further.".into(),
            );
        }
        let info = s.infos.get(&key).ok_or("Missing remote buffer.")?;
        if info.revision != expected_revision {
            return Err("This remote buffer is stale. Reopen it before editing; its text has been preserved.".into());
        }
        let sequence = info.revision + 1;
        let request = Request::Apply {
            session: s.session.clone(),
            epoch: s.epoch,
            document: info.document,
            sequence,
            base_revision: info.revision,
            action,
        };
        self.tx
            .try_send(Work::Edit(request.clone(), key.clone()))
            .map_err(|_| "Remote edit queue is full.")?;
        s.pending.push((request, key.clone()));
        s.pending_bytes += cost;
        if s.oldest.is_none() {
            s.oldest = Some(Instant::now());
        }
        s.infos.get_mut(&key).unwrap().revision = sequence;
        drop(s);
        Ok(())
    }
    fn map_host_path(&self, value: &mut Value) -> Result<(), String> {
        let text = value.as_str().ok_or("Invalid remote filesystem path.")?;
        let actual = Path::new(text);
        let root = Path::new(&self.config.root);
        let relative = actual
            .strip_prefix(root)
            .map_err(|_| "The host returned a path outside the SSH workspace.")?;
        let uri = self.uri(&wire_path::bytes(relative));
        self.relative(&uri)?;
        *value = Value::String(uri.to_string_lossy().into_owned());
        Ok(())
    }
    fn map_result(&self, value: &mut Value) -> Result<(), String> {
        match value {
            Value::Array(items) => {
                for value in items {
                    self.map_result(value)?;
                }
            }
            Value::Object(items) => {
                for (key, value) in items {
                    if matches!(key.as_str(), "root" | "path") && value.is_string() {
                        self.map_host_path(value)?;
                    } else if key == "root" && value.is_array() {
                        let bytes: Vec<u8> = serde_json::from_value(value.clone())
                            .map_err(|_| "Invalid Git root.")?;
                        let actual = wire_path::from_bytes(bytes);
                        let relative = actual
                            .strip_prefix(&self.config.root)
                            .map_err(|_| "Git repository is outside the SSH workspace.")?;
                        let uri = self.uri(&wire_path::bytes(relative));
                        self.relative(&uri)?;
                        *value = serde_json::to_value(wire_path::bytes(&uri))
                            .map_err(|e| e.to_string())?;
                    } else if matches!(
                        key.as_str(),
                        "entries" | "hits" | "files" | "plans" | "restore"
                    ) {
                        self.map_result(value)?;
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn start_process(
        &self,
        dir: &Path,
        command: Option<String>,
        file: Option<Vec<u8>>,
    ) -> Result<(Client, u64), String> {
        if !self.state.lock().unwrap().trusted {
            return Err("Trust this SSH workspace before running Git, builds, or a shell.".into());
        }
        let mut client = self.service()?;
        let s = self.state.lock().unwrap();
        let request = Request::ProcessStart {
            session: s.session.clone(),
            epoch: s.epoch,
            directory: self.relative(dir)?,
            command,
            file,
            cols: 80,
            rows: 24,
        };
        drop(s);
        let Response::Started { id } = rpc(&mut client, request)? else {
            return Err("Cannot start remote process.".into());
        };
        Ok((client, id))
    }
}
fn edit_cost(request: &Request) -> usize {
    match request {
        Request::Apply {
            action: Action::Edit { text, .. } | Action::EditBytes { text, .. },
            ..
        } => text.len() + 128,
        _ => 128,
    }
}
impl Backend for Connection {
    fn request(&self, mut call: Call, cancel: &AtomicBool) -> Result<Value, String> {
        if call.needs_execution() && !self.state.lock().unwrap().trusted {
            return Err("Trust this SSH workspace before running Git, builds, or a shell.".into());
        }
        let path_result = matches!(
            &call,
            Call::File {
                query: hedit_core::backend::FsQuery::Entry
                    | hedit_core::backend::FsQuery::Create { .. }
                    | hedit_core::backend::FsQuery::Rename(_)
                    | hedit_core::backend::FsQuery::Move(_)
                    | hedit_core::backend::FsQuery::DuplicateSibling(_),
                ..
            } | Call::Git {
                query: hedit_core::backend::GitQuery::Discover
                    | hedit_core::backend::GitQuery::Clone(_),
                ..
            }
        );
        call.map_paths(|path| self.relative(path).map(wire_path::from_bytes))?;
        let mut client = self.service()?;
        let s = self.state.lock().unwrap();
        let request = Request::TaskStart {
            session: s.session.clone(),
            epoch: s.epoch,
            call,
        };
        drop(s);
        let Response::Started { id } = rpc(&mut client, request)? else {
            return Err("Cannot start remote operation.".into());
        };
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = rpc(&mut client, Request::TaskCancel { id });
                return Err("Remote operation cancelled; refresh to inspect its outcome.".into());
            }
            match rpc(&mut client, Request::TaskPoll { id })? {
                Response::TaskResult { result } => {
                    let mut value = result?;
                    if path_result {
                        self.map_host_path(&mut value)?;
                    } else {
                        self.map_result(&mut value)?;
                    }
                    return Ok(value);
                }
                Response::Pending {} => std::thread::sleep(Duration::from_millis(20)),
                _ => return Err("Invalid remote task reply.".into()),
            }
        }
    }
    fn document(&self, path: &Path, change: DocumentChange) -> Result<(), String> {
        let (action, expected) = match change {
            DocumentChange::Edit {
                expected_revision,
                start,
                end,
                text,
            } => (Action::EditBytes { start, end, text }, expected_revision),
            DocumentChange::Format {
                expected_revision,
                encoding,
                bom,
            } => (Action::Format { encoding, bom }, expected_revision),
        };
        self.action(path, action, expected)
    }
    fn save(
        &self,
        path: &Path,
        target: Option<&Path>,
        expected_revision: u64,
    ) -> Result<(), String> {
        self.flush()?;
        let info = self.info(path)?;
        if info.revision != expected_revision {
            return Err("This remote view is stale. Its text is preserved; reopen it before changing host state.".into());
        }
        let s = self.state.lock().unwrap();
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| "Cannot generate save identity.")?;
        let request = Request::Save {
            session: s.session.clone(),
            epoch: s.epoch,
            document: info.document,
            sequence: info.revision,
            operation: nonce.iter().map(|b| format!("{b:02x}")).collect(),
            target: target.map(|p| self.relative(p)).transpose()?,
        };
        drop(s);
        let Response::Document { info } = self.call(request)? else {
            return Err("Invalid saved document receipt.".into());
        };
        let mut s = self.state.lock().unwrap();
        s.infos.remove(&path.to_string_lossy().into_owned());
        s.infos
            .insert(self.uri(&info.path).to_string_lossy().into_owned(), info);
        Ok(())
    }
    fn history(
        &self,
        path: &Path,
        redo: bool,
        expected_revision: u64,
    ) -> Result<RemoteBuffer, String> {
        self.flush()?;
        let info = self.info(path)?;
        if info.revision != expected_revision {
            return Err("This remote view is stale. Its text is preserved; reopen it before changing host state.".into());
        }
        let s = self.state.lock().unwrap();
        let request = Request::Apply {
            session: s.session.clone(),
            epoch: s.epoch,
            document: info.document,
            sequence: info.revision + 1,
            base_revision: info.revision,
            action: if redo {
                Action::Redo {}
            } else {
                Action::Undo {}
            },
        };
        drop(s);
        let Response::Durable { info, .. } = self.call(request)? else {
            return Err("Invalid history receipt.".into());
        };
        self.state
            .lock()
            .unwrap()
            .infos
            .insert(path.to_string_lossy().into_owned(), info.clone());
        self.read(info)
    }
    fn reload(
        &self,
        path: &Path,
        discard: bool,
        expected: Option<&str>,
        expected_revision: u64,
    ) -> Result<RemoteBuffer, String> {
        self.flush()?;
        let info = self.info(path)?;
        if info.revision != expected_revision {
            return Err("This remote view is stale. Its text is preserved; reopen it before changing host state.".into());
        }
        let s = self.state.lock().unwrap();
        let request = Request::Reload {
            session: s.session.clone(),
            epoch: s.epoch,
            document: info.document,
            discard,
            expected_revision,
            expected: expected.map(str::to_owned),
        };
        drop(s);
        let Response::Document { info } = self.call(request)? else {
            return Err("Invalid reload reply.".into());
        };
        self.state
            .lock()
            .unwrap()
            .infos
            .insert(path.to_string_lossy().into_owned(), info.clone());
        self.read(info)
    }
    fn retarget(&self, from: &Path, to: &Path) -> Result<(), String> {
        self.relative(to)?;
        let mut s = self.state.lock().unwrap();
        let mut info = s
            .infos
            .remove(&from.to_string_lossy().into_owned())
            .ok_or("Remote buffer missing.")?;
        info.path = self.relative(to)?;
        s.infos.insert(to.to_string_lossy().into_owned(), info);
        Ok(())
    }
    fn changed(&self, path: &Path) -> (bool, String) {
        let s = self.state.lock().unwrap();
        let changed = s
            .infos
            .get(&path.to_string_lossy().into_owned())
            .and_then(|info| s.changed.get(&info.document))
            .copied()
            .unwrap_or(false);
        (
            changed,
            if changed {
                "The file changed on the SSH host. Saving will preserve unseen external changes."
                    .into()
            } else {
                String::new()
            },
        )
    }
    fn build(
        &self,
        dir: &Path,
        command: &str,
        variables: Option<&hedit_core::build::Variables>,
        wake: Box<dyn Fn() + Send>,
    ) -> Result<RemoteBuild, String> {
        let file = variables.map(|v| self.relative(&v.file)).transpose()?;
        let connection = self.clone();
        let dir = dir.to_owned();
        let command = command.to_owned();
        let (tx, events) = mpsc::sync_channel(32);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            let (mut client, id) = match connection.start_process(&dir, Some(command), file) {
                Ok(value) => value,
                Err(e) => {
                    let _ = tx.send(hedit_core::build::Event::Output(e));
                    let _ = tx.send(hedit_core::build::Event::Finished(None, start.elapsed()));
                    wake();
                    return;
                }
            };
            loop {
                if stop.load(Ordering::Relaxed) {
                    let _ = rpc(&mut client, Request::ProcessStop { id });
                    let _ = tx.send(hedit_core::build::Event::Finished(None, start.elapsed()));
                    wake();
                    break;
                }
                match rpc(&mut client, Request::ProcessPoll { id }) {
                    Ok(Response::Process { bytes, done, exit }) => {
                        if !bytes.is_empty()
                            && tx
                                .send(hedit_core::build::Event::Output(
                                    String::from_utf8_lossy(&bytes).into_owned(),
                                ))
                                .is_err()
                        {
                            let _ = rpc(&mut client, Request::ProcessStop { id });
                            break;
                        }
                        wake();
                        if done {
                            let _ =
                                tx.send(hedit_core::build::Event::Finished(exit, start.elapsed()));
                            wake();
                            break;
                        }
                    }
                    other => {
                        let _ = tx.send(hedit_core::build::Event::Output(format!(
                            "Remote stream interrupted: {:?}",
                            other.err()
                        )));
                        let _ = tx.send(hedit_core::build::Event::Finished(None, start.elapsed()));
                        wake();
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        });
        Ok(RemoteBuild {
            events,
            cancel: Arc::new(move || cancelled.store(true, Ordering::Relaxed)),
        })
    }
    fn terminal(&self, dir: &Path) -> Result<RemotePty, String> {
        let connection = self.clone();
        let dir = dir.to_owned();
        let (input, rx) = mpsc::sync_channel::<Vec<u8>>(32);
        let (tx, output) = mpsc::sync_channel(32);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let dimensions = Arc::new(Mutex::new(None));
        let sizes = dimensions.clone();
        std::thread::spawn(move || {
            let (mut client, id) = match connection.start_process(&dir, None, None) {
                Ok(value) => value,
                Err(e) => {
                    let _ = tx.send(PtyEvent::Error(e));
                    return;
                }
            };
            loop {
                if stop.load(Ordering::Relaxed) {
                    let _ = rpc(&mut client, Request::ProcessStop { id });
                    break;
                }
                let result = (|| {
                    if let Some((cols, rows)) = sizes.lock().unwrap().take() {
                        rpc(&mut client, Request::ProcessResize { id, cols, rows })?;
                    }
                    for _ in 0..8 {
                        match rx.try_recv() {
                            Ok(bytes) => {
                                for chunk in bytes.chunks(8192) {
                                    rpc(
                                        &mut client,
                                        Request::ProcessInput {
                                            id,
                                            bytes: chunk.to_vec(),
                                        },
                                    )?;
                                }
                            }
                            Err(mpsc::TryRecvError::Disconnected) => {
                                rpc(&mut client, Request::ProcessStop { id })?;
                                return Ok(true);
                            }
                            Err(_) => break,
                        }
                    }
                    let Response::Process { bytes, done, .. } =
                        rpc(&mut client, Request::ProcessPoll { id })?
                    else {
                        return Err("Invalid terminal stream.".into());
                    };
                    if !bytes.is_empty() {
                        tx.send(PtyEvent::Bytes(bytes))
                            .map_err(|_| "Terminal closed.")?;
                    }
                    if done {
                        let _ = tx.send(PtyEvent::Exit);
                    }
                    Ok(done)
                })();
                match result {
                    Ok(true) => break,
                    Ok(false) => (),
                    Err(e) => {
                        let _ = tx.send(PtyEvent::Error(e));
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        });
        Ok(RemotePty {
            input,
            output,
            resize: Arc::new(move |cols, rows, _, _| {
                *dimensions.lock().unwrap() = Some((cols.clamp(1, 512), rows.clamp(1, 256)));
                Ok(())
            }),
            stop: Arc::new(move || cancelled.store(true, Ordering::Relaxed)),
        })
    }
}

pub fn join(root: &Path, path: &Path) -> PathBuf {
    if !backend::is_remote(root) {
        return root.join(path);
    }
    let bytes = wire_path::bytes(path);
    let escaped = bytes
        .split(|b| *b == b'/')
        .map(|p| percent_encoding::percent_encode(p, URI_SET).to_string())
        .collect::<Vec<_>>()
        .join("/");
    root.join(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn disconnected() -> Connection {
        let (tx, _) = mpsc::sync_channel(1);
        Connection {
            config: Config {
                target: SshTarget {
                    host: "test".into(),
                    user: None,
                    port: 22,
                    config: None,
                    helper: "/helper".into(),
                    state: "/state".into(),
                },
                root: "/srv/work".into(),
                takeover: false,
            },
            prefix: "hedit-ssh://test/".into(),
            tx,
            state: Arc::new(Mutex::new(State {
                epoch: 0,
                token: String::new(),
                session: String::new(),
                connected: false,
                error: None,
                pending: vec![],
                pending_bytes: 0,
                oldest: None,
                infos: HashMap::new(),
                changed: HashMap::new(),
                trusted: false,
            })),
        }
    }
    #[test]
    fn host_paths_cannot_become_local_filesystem_capabilities() {
        let remote = disconnected();
        for path in [
            "/Users/client/private",
            "/srv/work-other/file",
            "../../private",
            "/srv/work/../private",
            "hedit-ssh://other/file",
        ] {
            let mut value = serde_json::json!({"path":path,"entries":[]});
            assert!(remote.map_result(&mut value).is_err(), "{path}");
        }
        let mut value = serde_json::json!({"path":"/srv/work","entries":[{"path":"/srv/work/a %41.rs","name":"a %41.rs"}]});
        remote.map_result(&mut value).unwrap();
        assert_eq!(value["entries"][0]["path"], "hedit-ssh://test/a%20%2541.rs");
        let mut message = Value::String("/srv/work is text in a Git commit message".into());
        remote.map_result(&mut message).unwrap();
        assert_eq!(
            message.as_str(),
            Some("/srv/work is text in a Git commit message")
        );
    }
}
