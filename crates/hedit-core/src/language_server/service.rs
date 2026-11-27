use super::{
    client::{Client, ClientSettings},
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
    Rename,
    QuickFixes,
    SemanticTokens,
}
impl Feature {
    fn method(self) -> &'static str {
        match self {
            Self::Completion => "textDocument/completion",
            Self::Hover => "textDocument/hover",
            Self::Definition => "textDocument/definition",
            Self::References => "textDocument/references",
            Self::Formatting => "textDocument/formatting",
            Self::Rename => "textDocument/rename",
            Self::QuickFixes => "textDocument/codeAction",
            Self::SemanticTokens => "textDocument/semanticTokens/full",
        }
    }
    fn capability(self) -> &'static str {
        match self {
            Self::Completion => "completionProvider",
            Self::Hover => "hoverProvider",
            Self::Definition => "definitionProvider",
            Self::References => "referencesProvider",
            Self::Formatting => "documentFormattingProvider",
            Self::Rename => "renameProvider",
            Self::QuickFixes => "codeActionProvider",
            Self::SemanticTokens => "semanticTokensProvider",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    List,
    Install {
        id: String,
    },
    Detect {
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
    Rename {
        path: PathBuf,
        revision: u64,
        scalar: usize,
        name: String,
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
            Self::Request { path, .. } | Self::Rename { path, .. } => *path = map(path)?,
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
                manager::validate(registration)?;
            }
            Self::Detect { id }
            | Self::Install { id }
            | Self::Remove { id }
            | Self::Stop { id }
            | Self::Restart { id } => Manager::validate_id(id)?,
            Self::Rename { name, .. }
                if name.trim().is_empty()
                    || name.len() > 1024
                    || name.chars().any(char::is_control) =>
            {
                return Err("Invalid symbol name.".into());
            }
            Self::Request { path, scalar, .. } | Self::Rename { path, scalar, .. } => {
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
                        let request = matches!(
                            &envelope.operation,
                            Operation::Request { .. } | Operation::Rename { .. }
                        );
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
struct FeatureRequest {
    path: PathBuf,
    revision: u64,
    scalar: usize,
    feature: Feature,
    tab_size: u8,
    spaces: bool,
    name: Option<String>,
}

struct Session {
    client: Client,
    features: manager::Features,
    priority: i16,
    documents: BTreeMap<PathBuf, Document>,
    diagnostics: BTreeMap<PathBuf, (u64, Vec<Value>)>,
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
    fn request(&mut self, request: FeatureRequest, cancel: &AtomicBool) -> Result<Value, String> {
        let FeatureRequest {
            path,
            revision,
            scalar,
            feature,
            tab_size,
            spaces,
            name,
        } = request;
        let selected = self
            .sessions
            .iter()
            .filter(|(_, s)| {
                s.documents.contains_key(&path)
                    && s.features.enabled(feature)
                    && s.client.supported(feature.capability())
                    && (feature != Feature::SemanticTokens || {
                        let full = &s.client.capabilities["semanticTokensProvider"]["full"];
                        full == true || full.is_object()
                    })
            })
            .min_by(|(ka, a), (kb, b)| b.priority.cmp(&a.priority).then_with(|| ka.cmp(kb)))
            .map(|(key, _)| key.clone());
        let Some(session) = selected.and_then(|key| self.sessions.get_mut(&key)) else {
            return Err("No running language server supports this action.".into());
        };
        let document = &session.documents[&path];
        if document.revision != revision {
            return Err("The document changed before the LSP request.".into());
        }
        let position = session.client.encoding.position(&document.text, scalar)?;
        let mut params = json!({"textDocument":{"uri":text::uri(&path)?},"position":position});
        if feature == Feature::SemanticTokens {
            params = json!({"textDocument":{"uri":text::uri(&path)?}});
        }
        if feature == Feature::References {
            params["context"] = json!({"includeDeclaration":true});
        }
        if feature == Feature::Completion {
            params["context"] = json!({"triggerKind":1});
        }
        if feature == Feature::Formatting {
            params = json!({"textDocument":{"uri":text::uri(&path)?},"options":{"tabSize":tab_size.clamp(1,8),"insertSpaces":spaces}});
        }
        if feature == Feature::Rename {
            params["newName"] = json!(name.ok_or("Missing symbol name.")?);
        }
        if feature == Feature::QuickFixes {
            params = json!({"textDocument":{"uri":text::uri(&path)?},"range":{"start":position,"end":position},"context":{"diagnostics":session.diagnostics.get(&path).filter(|(r,_)|*r==revision).map(|(_,v)|v.iter().filter(|d|serde_json::from_value::<text::Range>(d["range"].clone()).is_ok_and(|r|(r.start.line,r.start.character)<=(position.line,position.character)&&(position.line,position.character)<=(r.end.line,r.end.character))).cloned().collect::<Vec<_>>()).unwrap_or_default(),"only":["quickfix"],"triggerKind":1}});
        }
        let root = document.root.clone();
        let timeout = if feature == Feature::SemanticTokens {
            Duration::from_millis(800)
        } else {
            Duration::from_secs(15)
        };
        let mut value = session
            .client
            .request(feature.method(), params, cancel, timeout)?;
        if feature == Feature::SemanticTokens {
            value = serde_json::to_value(text::semantic_tokens(
                &document.text,
                session.client.encoding,
                &session.client.capabilities["semanticTokensProvider"],
                &value,
            )?)
            .map_err(super::error)?;
        }
        if matches!(feature, Feature::Definition | Feature::References) {
            normalize_locations(&mut value, session.client.encoding, &session.documents)?;
        }
        if feature == Feature::Rename {
            value = serde_json::to_value(super::workspace_edit::prepare(
                value,
                session.client.encoding,
                &root,
                &self.documents,
            )?)
            .map_err(super::error)?;
        } else if feature == Feature::QuickFixes {
            value = serde_json::to_value(super::workspace_edit::quick_fixes(
                value,
                session.client.encoding,
                &root,
                &self.documents,
            )?)
            .map_err(super::error)?;
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
            Operation::Detect { id } => {
                serde_json::to_value(manager::detect(&id)?).map_err(super::error)
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
            } => self.request(
                FeatureRequest {
                    path,
                    revision,
                    scalar,
                    feature,
                    tab_size,
                    spaces,
                    name: None,
                },
                cancel,
            ),
            Operation::Rename {
                path,
                revision,
                scalar,
                name,
            } => self.request(
                FeatureRequest {
                    path,
                    revision,
                    scalar,
                    feature: Feature::Rename,
                    tab_size: 4,
                    spaces: true,
                    name: Some(name),
                },
                cancel,
            ),
        }
    }
    fn sync(
        &mut self,
        documents: Vec<Document>,
        cancel: &AtomicBool,
        emit: &impl Fn(Event) -> bool,
    ) -> Result<(), String> {
        let registered = self.manager.list()?;
        let mut wanted: HashMap<(String, PathBuf), Vec<usize>> = HashMap::new();
        for (index, document) in documents.iter().enumerate() {
            for entry in &registered {
                if self.stopped.contains(&entry.id)
                    || !entry.registration.features.any_enabled()
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
                let root = project_root(document, &entry.registration.root_markers);
                wanted
                    .entry((entry.id.clone(), root))
                    .or_default()
                    .push(index);
            }
        }
        // Retain crash suppression for all still-open projects, including ones
        // temporarily excluded by the session cap. A cap change is not a restart.
        self.failed.retain(|key| wanted.contains_key(key));
        let mut keys: Vec<_> = wanted
            .keys()
            .filter(|key| !self.failed.contains(*key))
            .cloned()
            .collect();
        let priority = |id: &str| {
            registered
                .iter()
                .find(|r| r.id == id)
                .map_or(0, |r| r.registration.priority)
        };
        keys.sort_by(|a, b| {
            wanted[a][0]
                .cmp(&wanted[b][0])
                .then_with(|| priority(&b.0).cmp(&priority(&a.0)))
                .then_with(|| a.cmp(b))
        });
        let selected_keys: Vec<_> = keys.iter().take(8).cloned().collect();
        let selected: HashSet<_> = selected_keys.iter().cloned().collect();
        for key in keys.into_iter().skip(8) {
            emit(Self::status(
                &key,
                "Limited",
                "At most eight language-server sessions can run per workspace.",
            ));
        }
        wanted.retain(|key, _| selected.contains(key));
        self.documents = documents;
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
        for key in selected_keys {
            let docs = wanted.remove(&key).unwrap();
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
                let registration = &registered
                    .iter()
                    .find(|r| r.id == key.0)
                    .unwrap()
                    .registration;
                let settings = ClientSettings {
                    configuration: registration.configuration.clone(),
                    initialization_options: registration
                        .initialization_options
                        .clone()
                        .unwrap_or_else(|| registration.configuration.clone()),
                    environment: registration.environment.clone(),
                    section: key.0.clone(),
                };
                let started = self.manager.launch(&key.0).and_then(|(launch, lease)| {
                    Client::start_owned(
                        launch,
                        lease,
                        &key.1,
                        settings,
                        cancel,
                        self.processes.clone(),
                    )
                });
                match started {
                    Ok(client) => {
                        self.sessions.insert(
                            key.clone(),
                            Session {
                                client,
                                features: registration.features.clone(),
                                priority: registration.priority,
                                documents: BTreeMap::new(),
                                diagnostics: BTreeMap::new(),
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
            // Clone text only for the bounded set of selected sessions.
            let docs = docs
                .into_iter()
                .map(|index| self.documents[index].clone())
                .collect();
            let encoding = session.client.encoding;
            let result = sync_session(session, docs, |document| {
                emit(Event::Diagnostics {
                    server: key.0.clone(),
                    root: key.1.clone(),
                    path: document.path.clone(),
                    revision: document.revision,
                    encoding,
                    items: vec![],
                });
            });
            session.diagnostics.retain(|path, (revision, _)| {
                session
                    .documents
                    .get(path)
                    .is_some_and(|d| d.revision == *revision)
            });
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
                if !session.features.diagnostics && !session.features.quick_fixes {
                    continue;
                }
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
                let items: Vec<Value> = params["diagnostics"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .take(1000)
                            .filter(|d| serde_json::to_vec(d).is_ok_and(|s| s.len() <= 8192))
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                let mut budget = 128 * 1024usize;
                let context = items
                    .iter()
                    .take(100)
                    .take_while(|item| {
                        let size = serde_json::to_vec(item).map_or(usize::MAX, |s| s.len());
                        if size > budget {
                            return false;
                        }
                        budget -= size;
                        true
                    })
                    .cloned()
                    .collect();
                session
                    .diagnostics
                    .insert(document.path.clone(), (document.revision, context));
                if session.features.diagnostics {
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
        }
        for key in failed {
            self.sessions.remove(&key);
            self.failed.insert(key);
        }
    }
}
fn project_root(document: &Document, custom: &[String]) -> PathBuf {
    let markers: &[&str] = match document.language.as_str() {
        "rust" => &["Cargo.toml", "rust-project.json"],
        "c" | "cpp" | "cuda" => &["compile_commands.json", "compile_flags.txt", ".clangd"],
        "python" => &["pyproject.toml", "setup.py", "ruff.toml", ".ruff.toml"],
        "go" => &["go.work", "go.mod"],
        "lua" => &[".luarc.json", ".luarc.jsonc"],
        "toml" => &["tombi.toml", "taplo.toml", ".taplo.toml", "Cargo.toml"],
        "javascript" | "typescript" | "jsx" | "tsx" => {
            &["tsconfig.json", "jsconfig.json", "package.json"]
        }
        _ => &[],
    };
    let mut at = document.path.parent();
    while let Some(path) = at {
        if !path.starts_with(&document.root) {
            break;
        }
        let found = if custom.is_empty() {
            markers.iter().any(|marker| path.join(marker).exists())
        } else {
            custom.iter().any(|marker| path.join(marker).exists())
        };
        if found {
            return path.to_owned();
        }
        if path == document.root {
            break;
        }
        at = path.parent();
    }
    document.root.clone()
}
fn language_id(document: &Document) -> &str {
    let extension = document
        .path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    match document.language.as_str() {
        "javascript" if extension.eq_ignore_ascii_case("jsx") => "javascriptreact",
        "typescript" if extension.eq_ignore_ascii_case("tsx") => "typescriptreact",
        "shell" => "shellscript",
        language => language,
    }
}
fn sync_session(
    session: &mut Session,
    documents: Vec<Document>,
    clear_diagnostics: impl Fn(&Document),
) -> Result<(), String> {
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
        if let Some(document) = session.documents.remove(&path) {
            clear_diagnostics(&document);
        }
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
            if let Some(previous) = session.documents.remove(&document.path) {
                clear_diagnostics(&previous);
            }
        }
        match session.documents.get(&document.path) {
            None=>session.client.notify("textDocument/didOpen",json!({"textDocument":{"uri":uri,"languageId":language_id(&document),"version":document.revision,"text":document.text}}))?,
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
    #[test]
    fn project_markers_respect_workspace_boundaries_and_protocol_language_ids() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(root.join("nested/src")).unwrap();
        std::fs::create_dir(root.join("nested/.project")).unwrap();
        std::fs::write(directory.path().join("outside.marker"), "").unwrap();
        let mut document = Document {
            path: root.join("nested/src/component.JSX"),
            root: root.clone(),
            language: "javascript".into(),
            revision: 0,
            text: String::new(),
            dirty: false,
        };
        assert_eq!(
            project_root(&document, &[".project".into()]),
            root.join("nested")
        );
        assert_eq!(project_root(&document, &["outside.marker".into()]), root);
        assert_eq!(language_id(&document), "javascriptreact");
        document.path.set_extension("TSX");
        document.language = "typescript".into();
        assert_eq!(language_id(&document), "typescriptreact");
        document.language = "shell".into();
        assert_eq!(language_id(&document), "shellscript");
    }
}
