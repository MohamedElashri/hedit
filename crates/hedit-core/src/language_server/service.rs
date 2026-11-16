use super::{
    client::Client,
    manager::{self, Manager, Registration},
    text::{self, Encoding, Position},
    MAX_DOCUMENT,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub path: PathBuf,
    pub root: PathBuf,
    pub language: String,
    pub revision: u64,
    pub text: String,
    pub dirty: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Feature {
    Completion,
    Hover,
    Definition,
    References,
    Formatting,
}
impl Feature {
    fn method(self) -> &'static str {
        match self {
            Self::Completion => "textDocument/completion",
            Self::Hover => "textDocument/hover",
            Self::Definition => "textDocument/definition",
            Self::References => "textDocument/references",
            Self::Formatting => "textDocument/formatting",
        }
    }
    fn capability(self) -> &'static str {
        match self {
            Self::Completion => "completionProvider",
            Self::Hover => "hoverProvider",
            Self::Definition => "definitionProvider",
            Self::References => "referencesProvider",
            Self::Formatting => "documentFormattingProvider",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    List,
    Install {
        id: String,
    },
    Remove {
        id: String,
    },
    Configure {
        id: String,
        registration: Box<Registration>,
    },
    Sync {
        documents: Vec<Document>,
    },
    Request {
        path: PathBuf,
        revision: u64,
        scalar: usize,
        feature: Feature,
        tab_size: u8,
        spaces: bool,
    },
    Restart {
        id: String,
    },
    Stop {
        id: String,
    },
}
impl Operation {
    pub fn map_paths(
        &mut self,
        mut map: impl FnMut(&Path) -> Result<PathBuf, String>,
    ) -> Result<(), String> {
        match self {
            Self::Sync { documents } => {
                for d in documents {
                    d.path = map(&d.path)?;
                    d.root = map(&d.root)?;
                }
            }
            Self::Request { path, .. } => *path = map(path)?,
            Self::Configure { registration, .. } => {
                registration.workspaces = std::mem::take(&mut registration.workspaces)
                    .into_iter()
                    .map(|(p, enabled)| {
                        Ok((map(Path::new(&p))?.to_string_lossy().into_owned(), enabled))
                    })
                    .collect::<Result<_, String>>()?;
            }
            // External launch paths are host paths entered explicitly by the
            // user. They must never be translated to client filesystem paths.
            _ => (),
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Sync { documents } => {
                if documents.len() > 32
                    || documents.iter().map(|d| d.text.len()).sum::<usize>() > 32 * 1024 * 1024
                {
                    return Err("Too many LSP document snapshots.".into());
                }
                let mut paths = HashSet::new();
                for d in documents {
                    if d.text.len() > MAX_DOCUMENT
                        || (!d.path.is_absolute()
                            && !d.path.to_string_lossy().starts_with("hedit-ssh:"))
                        || (!d.root.is_absolute()
                            && !d.root.to_string_lossy().starts_with("hedit-ssh:"))
                        || !d.path.starts_with(&d.root)
                        || d.revision > i64::MAX as u64
                        || d.language.len() > 80
                        || !paths.insert(&d.path)
                    {
                        return Err("Invalid LSP document snapshot.".into());
                    }
                }
            }
            Self::Configure { id, registration } => {
                Manager::validate_id(id)?;
                if serde_json::to_vec(registration)
                    .map_err(super::error)?
                    .len()
                    > 1024 * 1024
                {
                    return Err("Server configuration is too large.".into());
                }
            }
            Self::Install { id }
            | Self::Remove { id }
            | Self::Stop { id }
            | Self::Restart { id } => Manager::validate_id(id)?,
            Self::Request { path, scalar, .. } => {
                if (!path.is_absolute() && !path.to_string_lossy().starts_with("hedit-ssh:"))
                    || *scalar > MAX_DOCUMENT
                {
                    return Err("Invalid LSP request location.".into());
                }
            }
            Self::List => (),
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reply {
    pub path: PathBuf,
    pub revision: u64,
    pub encoding: Encoding,
    pub feature: Feature,
    pub value: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub id: String,
    pub root: PathBuf,
    pub state: String,
    pub detail: String,
    pub logs: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Disconnected(String),
    Result {
        ticket: u64,
        result: Result<Value, String>,
    },
    Response {
        ticket: u64,
        result: Result<Reply, String>,
    },
    Progress {
        id: String,
        message: String,
    },
    Status(Status),
    Diagnostics {
        server: String,
        root: PathBuf,
        path: PathBuf,
        revision: u64,
        encoding: Encoding,
        items: Vec<Value>,
    },
}
struct Envelope {
    ticket: u64,
    operation: Operation,
}
/// Dropping a service cancels downloads/requests and tears down all owned
/// children on its worker. No joins or filesystem operations run on the UI.
pub struct Service {
    input: mpsc::SyncSender<Envelope>,
    output: mpsc::Receiver<Event>,
    cancel: Arc<AtomicBool>,
    processes: Arc<super::client::Processes>,
}
impl Service {
    pub fn start(root: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (input, rx) = mpsc::sync_channel::<Envelope>(8);
        let (tx, output) = mpsc::sync_channel::<Event>(32);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let processes = Arc::new(super::client::Processes::default());
        let worker_processes = processes.clone();
        std::thread::spawn(move || {
            let emit = |event: Event| {
                let sent = tx.send(event).is_ok();
                wake();
                sent
            };
            let mut worker = Worker {
                manager: Manager::new(root),
                sessions: HashMap::new(),
                documents: vec![],
                failed: HashSet::new(),
                stopped: HashSet::new(),
                processes: worker_processes,
            };
            while !stop.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(40)) {
                    Ok(envelope) => {
                        let ticket = envelope.ticket;
                        let request = matches!(&envelope.operation, Operation::Request { .. });
                        let result = worker.run(envelope.operation, &stop, &emit);
                        let event = if request {
                            Event::Response {
                                ticket,
                                result: result
                                    .and_then(|v| serde_json::from_value(v).map_err(super::error)),
                            }
                        } else {
                            Event::Result { ticket, result }
                        };
                        if !emit(event) {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                }
                worker.poll(&emit);
            }
            for (_, mut session) in worker.sessions.drain() {
                session.client.stop();
            }
        });
        Self {
            input,
            output,
            cancel,
            processes,
        }
    }
    /// Adapt the same bounded mailbox to an SSH service. The callback owns
    /// its authenticated connection and is dropped when this bridge ends.
    pub fn bridge(
        mut transport: impl FnMut(Option<(u64, Operation)>) -> Result<Vec<Event>, String>
            + Send
            + 'static,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (input, rx) = mpsc::sync_channel::<Envelope>(8);
        let (tx, output) = mpsc::sync_channel(32);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let operation = match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(e) => Some((e.ticket, e.operation)),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                match transport(operation) {
                    Ok(events) => {
                        for event in events {
                            if tx.send(event).is_err() {
                                return;
                            }
                            wake();
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Disconnected(e));
                        wake();
                        break;
                    }
                }
            }
        });
        Self {
            input,
            output,
            cancel,
            processes: Arc::new(super::client::Processes::default()),
        }
    }
    pub fn submit(&self, ticket: u64, operation: Operation) -> Result<(), String> {
        operation.validate()?;
        self.input
            .try_send(Envelope { ticket, operation })
            .map_err(|_| "Language-server worker is busy or disconnected.".into())
    }
    pub fn poll(&self) -> Vec<Event> {
        let mut events = vec![];
        for _ in 0..32 {
            match self.output.try_recv() {
                Ok(event) => events.push(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !events.iter().any(|e| matches!(e, Event::Disconnected(_)))
                        && !self.cancel.load(Ordering::Relaxed)
                    {
                        events.push(Event::Disconnected("Language-server worker disconnected. Refresh the manager to reconnect.".into()));
                    }
                    break;
                }
            }
        }
        events
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.processes.kill_all();
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.cancel();
    }
}
struct Session {
    client: Client,
    documents: BTreeMap<PathBuf, Document>,
}
struct Worker {
    manager: Manager,
    sessions: HashMap<(String, PathBuf), Session>,
    documents: Vec<Document>,
    failed: HashSet<(String, PathBuf)>,
    stopped: HashSet<String>,
    processes: Arc<super::client::Processes>,
}
impl Worker {
    fn status(key: &(String, PathBuf), state: &str, detail: &str) -> Event {
        Event::Status(Status {
            id: key.0.clone(),
            root: key.1.clone(),
            state: state.into(),
            detail: detail.chars().take(4096).collect(),
            logs: vec![],
        })
    }
    fn stop(&mut self, id: &str, emit: &impl Fn(Event) -> bool) {
        let keys: Vec<_> = self
            .sessions
            .keys()
            .filter(|(server, _)| server == id)
            .cloned()
            .collect();
        for key in keys {
            if let Some(mut session) = self.sessions.remove(&key) {
                session.client.stop();
                emit(Self::status(&key, "Stopped", ""));
            }
        }
        self.failed.retain(|(server, _)| server != id);
    }
    fn run(
        &mut self,
        op: Operation,
        cancel: &AtomicBool,
        emit: &impl Fn(Event) -> bool,
    ) -> Result<Value, String> {
        op.validate()?;
        match op {
            Operation::List => {
                for (key, session) in &self.sessions {
                    let logs = session
                        .client
                        .logs
                        .lock()
                        .map(|logs| logs.iter().cloned().collect())
                        .unwrap_or_default();
                    emit(Event::Status(Status {
                        id: key.0.clone(),
                        root: key.1.clone(),
                        state: "Running".into(),
                        detail: String::new(),
                        logs,
                    }));
                }
                Ok(
                    json!({"catalog":manager::catalog(),"installed":self.manager.list()?,"directory":self.manager.root()}),
                )
            }
            Operation::Install { id } => {
                self.stop(&id, emit);
                self.manager.install(&id, cancel, |message| {
                    emit(Event::Progress {
                        id: id.clone(),
                        message: message.into(),
                    });
                })?;
                self.stopped.remove(&id);
                self.sync(self.documents.clone(), cancel, emit)?;
                Ok(json!(true))
            }
            Operation::Remove { id } => {
                self.stop(&id, emit);
                self.stopped.insert(id.clone());
                self.manager.remove(&id)?;
                Ok(json!(true))
            }
            Operation::Configure { id, registration } => {
                self.stop(&id, emit);
                self.manager.configure(&id, *registration)?;
                self.stopped.remove(&id);
                self.sync(self.documents.clone(), cancel, emit)?;
                Ok(json!(true))
            }
            Operation::Sync { documents } => {
                self.sync(documents, cancel, emit)?;
                Ok(json!(true))
            }
            Operation::Stop { id } => {
                self.stop(&id, emit);
                self.stopped.insert(id);
                Ok(json!(true))
            }
            Operation::Restart { id } => {
                self.stop(&id, emit);
                self.stopped.remove(&id);
                self.sync(self.documents.clone(), cancel, emit)?;
                Ok(json!(true))
            }
            Operation::Request {
                path,
                revision,
                scalar,
                feature,
                tab_size,
                spaces,
            } => {
                let Some(session) = self.sessions.values_mut().find(|s| {
                    s.documents.contains_key(&path) && s.client.supported(feature.capability())
                }) else {
                    return Err("No running language server supports this action.".into());
                };
                let document = &session.documents[&path];
                if document.revision != revision {
                    return Err("The document changed before the LSP request.".into());
                }
                let position = session.client.encoding.position(&document.text, scalar)?;
                let mut params =
                    json!({"textDocument":{"uri":text::uri(&path)?},"position":position});
                if feature == Feature::References {
                    params["context"] = json!({"includeDeclaration":true});
                }
                if feature == Feature::Completion {
                    params["context"] = json!({"triggerKind":1});
                }
                if feature == Feature::Formatting {
                    params = json!({"textDocument":{"uri":text::uri(&path)?},"options":{"tabSize":tab_size.clamp(1,8),"insertSpaces":spaces}});
                }
                let result = session.client.request(
                    feature.method(),
                    params,
                    cancel,
                    Duration::from_secs(15),
                );
                let mut value = result?;
                if matches!(feature, Feature::Definition | Feature::References) {
                    normalize_locations(&mut value, session.client.encoding, &session.documents)?;
                }
                serde_json::to_value(Reply {
                    path,
                    revision,
                    encoding: session.client.encoding,
                    feature,
                    value,
                })
                .map_err(super::error)
            }
        }
    }
    fn sync(
        &mut self,
        documents: Vec<Document>,
        cancel: &AtomicBool,
        emit: &impl Fn(Event) -> bool,
    ) -> Result<(), String> {
        let registered = self.manager.list()?;
        let mut wanted: HashMap<(String, PathBuf), Vec<Document>> = HashMap::new();
        for document in &documents {
            for entry in &registered {
                if self.stopped.contains(&entry.id)
                    || !entry.registration.enabled_at(&document.root)
                    || !entry
                        .registration
                        .languages
                        .iter()
                        .any(|l| l == &document.language)
                    || (entry.registration.external.is_none()
                        && entry.registration.managed.is_none())
                {
                    continue;
                }
                let root = project_root(document);
                wanted
                    .entry((entry.id.clone(), root))
                    .or_default()
                    .push(document.clone());
                // One selected server per document avoids duplicate diagnostics
                // and ambiguous formatting. Ordering is deterministic by ID.
                break;
            }
        }
        self.documents = documents;
        self.failed.retain(|key| wanted.contains_key(key));
        let obsolete: Vec<_> = self
            .sessions
            .keys()
            .filter(|k| !wanted.contains_key(*k))
            .cloned()
            .collect();
        for key in obsolete {
            if let Some(mut s) = self.sessions.remove(&key) {
                s.client.stop();
                emit(Self::status(&key, "Stopped", ""));
            }
        }
        for (key, docs) in wanted {
            manager::check_cancel(cancel)?;
            if !self.sessions.contains_key(&key) && !self.failed.contains(&key) {
                if self.sessions.len() >= 8 {
                    emit(Self::status(
                        &key,
                        "Limited",
                        "At most eight language-server sessions can run per workspace.",
                    ));
                    continue;
                }
                emit(Self::status(&key, "Starting", ""));
                let config = registered
                    .iter()
                    .find(|r| r.id == key.0)
                    .unwrap()
                    .registration
                    .configuration
                    .clone();
                let started = self.manager.launch(&key.0).and_then(|(launch, lease)| {
                    Client::start_owned(
                        launch,
                        lease,
                        &key.1,
                        config,
                        cancel,
                        self.processes.clone(),
                        key.0.clone(),
                    )
                });
                match started {
                    Ok(client) => {
                        self.sessions.insert(
                            key.clone(),
                            Session {
                                client,
                                documents: BTreeMap::new(),
                            },
                        );
                        emit(Self::status(&key, "Running", ""));
                    }
                    Err(e) => {
                        self.failed.insert(key.clone());
                        emit(Self::status(&key, "Failed", &e));
                        continue;
                    }
                }
            }
            let Some(session) = self.sessions.get_mut(&key) else {
                continue;
            };
            let result = sync_session(session, docs);
            if let Err(e) = result {
                self.sessions.remove(&key);
                self.failed.insert(key.clone());
                emit(Self::status(&key, "Failed", &e));
            }
        }
        Ok(())
    }
    fn poll(&mut self, emit: &impl Fn(Event) -> bool) {
        let mut failed = vec![];
        for (key, session) in &mut self.sessions {
            if let Err(e) = session.client.poll() {
                let logs = session
                    .client
                    .logs
                    .lock()
                    .map(|l| l.iter().cloned().collect())
                    .unwrap_or_default();
                emit(Event::Status(Status {
                    id: key.0.clone(),
                    root: key.1.clone(),
                    state: "Failed".into(),
                    detail: e,
                    logs,
                }));
                failed.push(key.clone());
                continue;
            }
            while let Some(notification) = session.client.notifications.pop_front() {
                let params = &notification["params"];
                let Ok(path) = params["uri"]
                    .as_str()
                    .ok_or(())
                    .and_then(|uri| text::path(uri).map_err(|_| ()))
                else {
                    continue;
                };
                let Some(document) = session.documents.get(&path).or_else(|| {
                    session
                        .documents
                        .values()
                        .find(|d| text::same_path(&d.path, &path))
                }) else {
                    continue;
                };
                if params
                    .get("version")
                    .is_some_and(|v| !v.is_null() && v.as_u64() != Some(document.revision))
                {
                    continue;
                }
                let items = params["diagnostics"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .take(1000)
                            .filter(|d| serde_json::to_vec(d).is_ok_and(|s| s.len() <= 8192))
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                emit(Event::Diagnostics {
                    server: key.0.clone(),
                    root: key.1.clone(),
                    path: document.path.clone(),
                    revision: document.revision,
                    encoding: session.client.encoding,
                    items,
                });
            }
        }
        for key in failed {
            self.sessions.remove(&key);
            self.failed.insert(key);
        }
    }
}
fn project_root(document: &Document) -> PathBuf {
    let markers: &[&str] = match document.language.as_str() {
        "rust" => &["Cargo.toml", "rust-project.json"],
        "c" | "cpp" | "cuda" => &["compile_commands.json", "compile_flags.txt", ".clangd"],
        "python" => &["pyproject.toml", "setup.py"],
        _ => &[],
    };
    let mut at = document.path.parent();
    while let Some(path) = at {
        if !path.starts_with(&document.root) {
            break;
        }
        if markers.iter().any(|marker| path.join(marker).is_file()) {
            return path.to_owned();
        }
        if path == document.root {
            break;
        }
        at = path.parent();
    }
    document.root.clone()
}
fn sync_session(session: &mut Session, documents: Vec<Document>) -> Result<(), String> {
    let wanted: HashSet<_> = documents.iter().map(|d| d.path.clone()).collect();
    let closed: Vec<_> = session
        .documents
        .keys()
        .filter(|p| !wanted.contains(*p))
        .cloned()
        .collect();
    for path in closed {
        session.client.notify(
            "textDocument/didClose",
            json!({"textDocument":{"uri":text::uri(&path)?}}),
        )?;
        session.documents.remove(&path);
    }
    for document in documents {
        let uri = text::uri(&document.path)?;
        if session.documents.get(&document.path).is_some_and(|old| {
            old.language != document.language
                || document.revision < old.revision
                || (document.revision == old.revision && document.text != old.text)
        }) {
            // A close/reopen can be coalesced into one workspace snapshot.
            // Start a new LSP lifecycle before an editor revision is reused;
            // didChange versions must increase within an open document.
            session
                .client
                .notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))?;
            session.documents.remove(&document.path);
        }
        match session.documents.get(&document.path) {
            None=>session.client.notify("textDocument/didOpen",json!({"textDocument":{"uri":uri,"languageId":document.language,"version":document.revision,"text":document.text}}))?,
            Some(old)=>{
                if old.revision!=document.revision {
                    let sync=&session.client.capabilities["textDocumentSync"];
                    let kind=sync.as_u64().or_else(||sync["change"].as_u64()).unwrap_or(0);
                    if kind!=0{
                        let change=if kind==2{let end=session.client.encoding.position(&old.text,old.text.chars().count())?;json!({"range":{"start":Position::default(),"end":end},"text":document.text})}else{json!({"text":document.text})};
                        session.client.notify("textDocument/didChange",json!({"textDocument":{"uri":uri,"version":document.revision},"contentChanges":[change]}))?;
                    }
                }
                let save=&session.client.capabilities["textDocumentSync"]["save"];
                if !document.dirty && (old.dirty || old.revision!=document.revision) && !save.is_null() && save!=false {
                    let mut params=json!({"textDocument":{"uri":uri}});
                    if save["includeText"]==true{params["text"]=json!(document.text);}
                    session.client.notify("textDocument/didSave",params)?;
                }
            }
        }
        session.documents.insert(document.path.clone(), document);
    }
    Ok(())
}

/// Resolve encoded columns away from the UI before navigating to unopened files.
fn normalize_locations(
    value: &mut Value,
    encoding: Encoding,
    documents: &BTreeMap<PathBuf, Document>,
) -> Result<(), String> {
    let locations = match value {
        Value::Array(items) => items.as_mut_slice(),
        Value::Object(_) => std::slice::from_mut(value),
        _ => return Ok(()),
    };
    for location in locations.iter_mut().take(2000) {
        let uri = location["uri"]
            .as_str()
            .or_else(|| location["targetUri"].as_str());
        let Some(uri) = uri else { continue };
        let Ok(path) = text::path(uri) else { continue };
        let range = location
            .get("targetSelectionRange")
            .or_else(|| location.get("range"));
        let Some(range) = range else { continue };
        let Ok(range) = serde_json::from_value::<text::Range>(range.clone()) else {
            continue;
        };
        if encoding == Encoding::Utf16 {
            location["heditColumn16"] = json!(range.start.character as u64 + 1);
            continue;
        }
        let document = documents
            .get(&path)
            .or_else(|| documents.values().find(|d| text::same_path(&d.path, &path)));
        let loaded = if document.is_some() {
            None
        } else {
            crate::file_io::open_regular(&path)
                .and_then(|f| crate::file_io::read_bounded(f, MAX_DOCUMENT as u64))
                .ok()
                .and_then(|b| {
                    crate::text_encoding::decode(&b, crate::text_encoding::detect(&b))
                        .ok()
                        .map(|(text, _)| text)
                })
        };
        let source = document.map(|d| d.text.as_str()).or(loaded.as_deref());
        let Some(source) = source else {
            location["heditInvalidPosition"] = json!(true);
            continue;
        };
        match encoding.byte(source, range.start) {
            Ok(byte) => {
                let start = source[..byte].rfind('\n').map_or(0, |n| n + 1);
                location["heditColumn16"] = json!(source[start..byte].encode_utf16().count() + 1);
            }
            Err(_) => location["heditInvalidPosition"] = json!(true),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn diagnostics_reject_negative_and_mismatched_versions_but_accept_unversioned_results() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Manager::new(dir.path().join("servers"));
        manager
            .configure(
                "fixture",
                Registration {
                    languages: vec!["rust".into()],
                    external: Some(super::super::tests::launch(&[])),
                    ..Registration::default()
                },
            )
            .unwrap();
        let mut worker = Worker {
            manager,
            sessions: HashMap::new(),
            documents: vec![],
            failed: HashSet::new(),
            stopped: HashSet::new(),
            processes: Arc::new(super::super::client::Processes::default()),
        };
        let document = Document {
            path: dir.path().join("main.rs"),
            root: dir.path().into(),
            language: "rust".into(),
            revision: 0,
            text: "fo".into(),
            dirty: false,
        };
        let cancel = AtomicBool::new(false);
        worker
            .sync(vec![document.clone()], &cancel, &|_| true)
            .unwrap();
        for (version, accepted) in [
            (Some(json!(-1)), false),
            (Some(json!(1)), false),
            (Some(json!(0)), true),
            (Some(Value::Null), true),
            (None, true),
        ] {
            let mut params = json!({"uri":text::uri(&document.path).unwrap(),"diagnostics":[{"message":"version check"}]});
            if let Some(version) = &version {
                params["version"] = version.clone();
            }
            worker
                .sessions
                .values_mut()
                .next()
                .unwrap()
                .client
                .request("test/diagnostics", params, &cancel, Duration::from_secs(2))
                .unwrap();
            let events = RefCell::new(vec![]);
            worker.poll(&|e| {
                events.borrow_mut().push(e);
                true
            });
            let found = events.borrow().iter().any(|event| matches!(event, Event::Diagnostics{items,..} if items.iter().any(|d|d["message"]=="version check")));
            assert_eq!(found, accepted, "diagnostic version {version:?}");
        }
    }
}
