//! Session-scoped jobs and streams. Tool work never blocks edit commits.
use crate::{
    files::Files,
    host,
    journal::Journal,
    protocol::{self, Error, Request, Response, Result},
};
use hedit_core::backend::{Call, FsQuery, GitQuery};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
};

struct Job {
    connection: u64,
    epoch: u64,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<std::result::Result<serde_json::Value, String>>>>,
    retarget: Option<String>,
    published: bool,
}
enum Kind {
    Build(hedit_core::build::Running),
    Pty {
        master: Box<dyn MasterPty + Send>,
        child: Option<Box<dyn Child + Send + Sync>>,
        input: mpsc::SyncSender<Vec<u8>>,
        output: mpsc::Receiver<Vec<u8>>,
    },
}
struct Process {
    connection: u64,
    epoch: u64,
    kind: Kind,
    pending: std::collections::VecDeque<u8>,
    done: bool,
    exit: Option<i32>,
}
impl Process {
    fn stop(&mut self) {
        match &mut self.kind {
            Kind::Build(build) => build.cancel(),
            Kind::Pty { master, child, .. } => {
                if let Some(group) = master
                    .process_group_leader()
                    .filter(|g| *g > 1)
                    .and_then(rustix::process::Pid::from_raw)
                {
                    let _ =
                        rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
                }
                if let Some(mut child) = child.take() {
                    let _ = child.kill();
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
            }
        }
        self.done = true;
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if !self.done {
            self.stop()
        }
    }
}
pub struct Runtime {
    pub token: String,
    epoch: u64,
    primary: u64,
    bound: HashMap<u64, u64>,
    trusted: bool,
    next: u64,
    jobs: HashMap<u64, Job>,
    processes: HashMap<u64, Process>,
    uploads: HashMap<(u64, u64), (usize, Vec<u8>)>,
    downloads: HashMap<(u64, u64), Vec<u8>>,
    git_lock: Arc<Mutex<()>>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            token: String::new(),
            epoch: 0,
            primary: 0,
            bound: HashMap::new(),
            trusted: false,
            next: 1,
            jobs: HashMap::new(),
            processes: HashMap::new(),
            uploads: HashMap::new(),
            downloads: HashMap::new(),
            git_lock: Arc::new(Mutex::new(())),
        }
    }
}
impl Runtime {
    fn id(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        id
    }
    pub fn ready_for_owner(&self) -> bool {
        self.jobs
            .values()
            .all(|job| job.result.lock().is_ok_and(|value| value.is_some()))
    }
    pub fn owner(&mut self, connection: u64, epoch: u64) -> Result<()> {
        self.detach();
        self.jobs.clear();
        self.epoch = epoch;
        self.primary = connection;
        self.token = host::random_id()?;
        Ok(())
    }
    pub fn disconnected(&mut self, connection: u64) {
        self.processes
            .retain(|_, process| process.connection != connection);
        for job in self
            .jobs
            .values()
            .filter(|job| job.connection == connection)
        {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.uploads.retain(|(c, _), _| *c != connection);
        self.downloads.retain(|(c, _), _| *c != connection);
        self.bound.remove(&connection);
        if self.primary == connection {
            self.detach();
        }
    }
    fn detach(&mut self) {
        for job in self.jobs.values() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.processes.clear();
        self.bound.clear();
        self.trusted = false;
        self.primary = 0;
    }
    fn authorized(&self, connection: u64, epoch: u64) -> Result<()> {
        if epoch != self.epoch
            || self.primary == 0
            || (self.primary != connection && self.bound.get(&connection) != Some(&epoch))
        {
            return Err(Error::Fenced);
        }
        Ok(())
    }
    fn check_session(&self, journal: &Journal, session: &str) -> Result<()> {
        if journal.session() != session {
            Err(Error::Conflict)
        } else {
            Ok(())
        }
    }
    pub fn completed(&mut self, journal: &mut Journal) {
        for job in self.jobs.values_mut() {
            if job.published {
                continue;
            }
            if let Ok(mut result) = job.result.lock() {
                if result.is_some() {
                    if let Some(id) = &job.retarget {
                        if let Err(error) = journal.finish_move(id) {
                            *result = Some(Err(format!(
                                "The move completed but recovery paths need attention: {error}"
                            )));
                        }
                    }
                    job.published = true;
                }
            }
        }
        let primary = self.primary;
        let bound = &self.bound;
        self.jobs.retain(|_, job| {
            job.connection == primary
                || bound.contains_key(&job.connection)
                || job.result.lock().is_ok_and(|result| result.is_none())
        });
    }
    pub fn reply(&mut self, connection: u64, response: Result<Response>) -> Result<Response> {
        let response = response?;
        // Reserve the outer Reply envelope as well as the response payload.
        if protocol::encode(&response).is_ok_and(|bytes| bytes.len() <= protocol::MAX_FRAME - 512) {
            return Ok(response);
        }
        let bytes =
            protocol::encode_limit(&response, protocol::MAX_VALUE).map_err(|_| Error::Limit)?;
        if self.downloads.values().map(Vec::len).sum::<usize>() + bytes.len()
            > protocol::MAX_VALUE * 2
        {
            return Err(Error::Limit);
        }
        let id = self.id();
        let length = bytes.len() as u64;
        self.downloads.insert((connection, id), bytes);
        Ok(Response::Transfer { id, length })
    }
    pub fn uploaded(&mut self, connection: u64, id: u64) -> Result<Request> {
        let (length, bytes) = self
            .uploads
            .remove(&(connection, id))
            .ok_or(Error::Missing)?;
        if bytes.len() != length {
            return Err(Error::Protocol);
        }
        let request: Request = serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)?;
        if !matches!(
            request,
            Request::Apply { .. }
                | Request::TaskStart { .. }
                | Request::Save { .. }
                | Request::Metadata { .. }
                | Request::Reload { .. }
        ) {
            return Err(Error::Permission);
        }
        Ok(request)
    }
    pub fn handle(
        &mut self,
        journal: &mut Journal,
        connection: u64,
        request: &Request,
    ) -> Option<Result<Response>> {
        match request {
            Request::Changes { documents } => Some((|| {
                if documents.len() > 8 {
                    return Err(Error::Limit);
                }
                let values = documents
                    .iter()
                    .map(|id| journal.changed(*id).map(|changed| (*id, changed)))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Response::Value {
                    value: serde_json::to_value(values).map_err(|_| Error::Protocol)?,
                })
            })()),
            Request::Content { document } => Some((|| {
                Ok(Response::Buffer {
                    info: journal.info(*document)?,
                    text: journal.text(*document)?,
                })
            })()),
            Request::Bind {
                session,
                epoch,
                token,
            } => Some((|| {
                self.check_session(journal, session)?;
                if *epoch != self.epoch || token != &self.token || self.primary == 0 {
                    return Err(Error::Fenced);
                }
                self.bound.insert(connection, *epoch);
                Ok(Response::Pong {})
            })()),
            Request::Trust { epoch, allow } => Some((|| {
                self.authorized(connection, *epoch)?;
                if self.primary != connection {
                    return Err(Error::Fenced);
                }
                if !allow {
                    for job in self.jobs.values() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                    self.processes.clear();
                }
                self.trusted = *allow;
                Ok(Response::Pong {})
            })()),
            Request::UploadStart { length } => Some((|| {
                if *length == 0
                    || *length > protocol::MAX_VALUE as u64
                    || self.uploads.keys().any(|(c, _)| *c == connection)
                {
                    return Err(Error::Limit);
                }
                let id = self.id();
                self.uploads
                    .insert((connection, id), (*length as usize, Vec::new()));
                Ok(Response::Upload { id })
            })()),
            Request::UploadWrite { id, bytes } => Some((|| {
                if bytes.len() > 40 * 1024
                    || self.uploads.values().map(|(_, b)| b.len()).sum::<usize>() + bytes.len()
                        > protocol::MAX_VALUE * 2
                {
                    return Err(Error::Limit);
                }
                let (length, data) = self
                    .uploads
                    .get_mut(&(connection, *id))
                    .ok_or(Error::Missing)?;
                if data.len() + bytes.len() > *length {
                    return Err(Error::Limit);
                }
                data.extend_from_slice(bytes);
                Ok(Response::Pong {})
            })()),
            Request::Download { id, offset } => Some((|| {
                let data = self
                    .downloads
                    .get(&(connection, *id))
                    .ok_or(Error::Missing)?;
                let at = usize::try_from(*offset).map_err(|_| Error::Limit)?;
                if at > data.len() {
                    return Err(Error::Protocol);
                }
                let end = (at + 40 * 1024).min(data.len());
                let bytes = data[at..end].to_vec();
                let done = end == data.len();
                if done {
                    self.downloads.remove(&(connection, *id));
                }
                Ok(Response::Bytes { bytes, done })
            })()),
            Request::Save {
                session,
                epoch,
                document,
                sequence,
                operation,
                target,
            } => Some((|| {
                self.authorized(connection, *epoch)?;
                self.check_session(journal, session)?;
                Ok(Response::Document {
                    info: journal.save_document(
                        *document,
                        *sequence,
                        operation,
                        target.as_deref(),
                    )?,
                })
            })()),
            Request::Reload {
                session,
                epoch,
                document,
                discard,
                expected_revision,
                expected,
            } => Some((|| {
                self.authorized(connection, *epoch)?;
                self.check_session(journal, session)?;
                if journal.info(*document)?.revision != *expected_revision {
                    return Err(Error::Conflict);
                }
                Ok(Response::Document {
                    info: journal.reload_document(*document, *discard, expected.as_deref())?,
                })
            })()),
            Request::Scratch { epoch } => Some((|| {
                self.authorized(connection, *epoch)?;
                Ok(Response::Document {
                    info: journal.scratch()?,
                })
            })()),
            Request::Metadata { epoch, value } => Some((|| {
                self.authorized(connection, *epoch)?;
                Ok(Response::Value {
                    value: journal.metadata(value.as_ref())?,
                })
            })()),
            Request::New { epoch, path } => Some((|| {
                self.authorized(connection, *epoch)?;
                Ok(Response::Document {
                    info: journal.new_document(path)?,
                })
            })()),
            Request::TaskStart {
                session,
                epoch,
                call,
            } => Some((|| {
                self.authorized(connection, *epoch)?;
                self.check_session(journal, session)?;
                if self.jobs.len() >= 8 || (call.needs_execution() && !self.trusted) {
                    return Err(if !self.trusted {
                        Error::Permission
                    } else {
                        Error::Busy
                    });
                }
                let files = Files {
                    root: journal.root_file()?,
                    path: journal.root_path()?,
                    state: journal.state_path().to_owned(),
                };
                let dirty = journal.dirty_paths()?;
                if !dirty.is_empty() {
                    let blocked = match call {
                        Call::Git {
                            query: GitQuery::Execute(operation),
                            ..
                        } => {
                            operation.changes_worktree()
                                || matches!(
                                    operation,
                                    hedit_core::git::Operation::Stage(_)
                                        | hedit_core::git::Operation::Hunk { .. }
                                        | hedit_core::git::Operation::Lines { .. }
                                )
                        }
                        Call::Replace(plan) => dirty.contains(&Files::relative(&plan.path)?),
                        Call::Restore(record) => dirty.contains(&Files::relative(&record.path)?),
                        _ => false,
                    };
                    if blocked {
                        return Err(Error::Conflict);
                    }
                }
                let open = if matches!(call, Call::Search { .. }) {
                    let mut values = HashMap::new();
                    for info in journal.list()? {
                        if info.dirty {
                            values.insert(
                                files.path.join(host::path_or_empty(&info.path)),
                                journal.text(info.document)?,
                            );
                        }
                    }
                    values
                } else {
                    HashMap::new()
                };
                let retarget = match call {
                    Call::File {
                        path,
                        query: FsQuery::Rename(name),
                    } => Some((
                        Files::relative(path)?,
                        Files::relative(&path.parent().unwrap_or(Path::new("")).join(name))?,
                    )),
                    Call::File {
                        path,
                        query: FsQuery::Move(target),
                    } => Some((Files::relative(path)?, Files::relative(target)?)),
                    _ => None,
                };
                let retarget = retarget
                    .map(|(from, to)| journal.prepare_move(&from, &to))
                    .transpose()?;
                let id = self.id();
                let cancel = Arc::new(AtomicBool::new(false));
                let result = Arc::new(Mutex::new(None));
                let output = result.clone();
                let stop = cancel.clone();
                let call = call.clone();
                let git_lock = self.git_lock.clone();
                std::thread::spawn(move || {
                    let value = execute(files, call, open, stop, git_lock);
                    if let Ok(mut slot) = output.lock() {
                        *slot = Some(value);
                    }
                });
                self.jobs.insert(
                    id,
                    Job {
                        connection,
                        epoch: *epoch,
                        cancel,
                        result,
                        retarget,
                        published: false,
                    },
                );
                Ok(Response::Started { id })
            })()),
            Request::TaskPoll { id } => Some((|| {
                self.authorized(connection, self.epoch)?;
                self.completed(journal);
                let job = self.jobs.get(id).ok_or(Error::Missing)?;
                if job.epoch != self.epoch {
                    return Err(Error::Fenced);
                }
                let result = job.result.lock().map_err(|_| Error::Unavailable)?.take();
                if let Some(result) = result {
                    self.jobs.remove(id);
                    Ok(Response::TaskResult { result })
                } else {
                    Ok(Response::Pending {})
                }
            })()),
            Request::TaskCancel { id } => Some((|| {
                self.authorized(connection, self.epoch)?;
                let job = self.jobs.get(id).ok_or(Error::Missing)?;
                job.cancel.store(true, Ordering::Relaxed);
                Ok(Response::Pong {})
            })()),
            Request::ProcessStart {
                session,
                epoch,
                directory,
                command,
                file,
                cols,
                rows,
            } => Some((|| {
                self.authorized(connection, *epoch)?;
                self.check_session(journal, session)?;
                if !self.trusted {
                    return Err(Error::Permission);
                }
                if self.processes.len() >= 8 {
                    return Err(Error::Limit);
                }
                let files = Files {
                    root: journal.root_file()?,
                    path: journal.root_path()?,
                    state: journal.state_path().to_owned(),
                };
                let relative = host::path_or_empty(directory);
                let _ = files.directory(&relative)?;
                let actual = files.actual(&relative)?;
                let kind = if let Some(command) = command {
                    if command.len() > 32768 {
                        return Err(Error::Limit);
                    }
                    let variables = file.as_ref().map(|p| hedit_core::build::Variables {
                        file: files.path.join(host::path_or_empty(p)),
                        folder: files.path.clone(),
                    });
                    Kind::Build(
                        if let Some(v) = variables {
                            hedit_core::build::Running::start_with_variables(
                                command,
                                &actual,
                                &v,
                                || {},
                            )
                        } else {
                            hedit_core::build::Running::start(command, &actual, || {})
                        }
                        .map_err(|_| Error::Unavailable)?,
                    )
                } else {
                    let pair = portable_pty::native_pty_system()
                        .openpty(PtySize {
                            cols: (*cols).clamp(1, 512),
                            rows: (*rows).clamp(1, 256),
                            pixel_width: 0,
                            pixel_height: 0,
                        })
                        .map_err(|_| Error::Unavailable)?;
                    let mut builder = CommandBuilder::new_default_prog();
                    builder.cwd(actual);
                    builder.env("TERM", "xterm-256color");
                    builder.env("COLORTERM", "truecolor");
                    let child = pair
                        .slave
                        .spawn_command(builder)
                        .map_err(|_| Error::Unavailable)?;
                    drop(pair.slave);
                    let mut reader = pair
                        .master
                        .try_clone_reader()
                        .map_err(|_| Error::Unavailable)?;
                    let mut writer = pair.master.take_writer().map_err(|_| Error::Unavailable)?;
                    let (out_tx, output) = mpsc::sync_channel(32);
                    let (input, in_rx) = mpsc::sync_channel::<Vec<u8>>(32);
                    std::thread::spawn(move || {
                        let mut buffer = [0; 8192];
                        while let Ok(n) = reader.read(&mut buffer) {
                            if n == 0 || out_tx.send(buffer[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    });
                    std::thread::spawn(move || {
                        for bytes in in_rx {
                            if writer
                                .write_all(&bytes)
                                .and_then(|_| writer.flush())
                                .is_err()
                            {
                                break;
                            }
                        }
                    });
                    Kind::Pty {
                        master: pair.master,
                        child: Some(child),
                        input,
                        output,
                    }
                };
                let id = self.id();
                self.processes.insert(
                    id,
                    Process {
                        connection,
                        epoch: *epoch,
                        kind,
                        pending: Default::default(),
                        done: false,
                        exit: None,
                    },
                );
                Ok(Response::Started { id })
            })()),
            Request::ProcessPoll { id } => Some((|| {
                self.authorized(connection, self.epoch)?;
                let p = self.processes.get_mut(id).ok_or(Error::Missing)?;
                if p.epoch != self.epoch {
                    return Err(Error::Fenced);
                }
                if p.pending.len() < 8192 {
                    match &mut p.kind {
                        Kind::Build(build) => {
                            while p.pending.len() < 8192 {
                                match build.events.try_recv() {
                                    Ok(hedit_core::build::Event::Output(text)) => {
                                        p.pending.extend(text.as_bytes())
                                    }
                                    Ok(hedit_core::build::Event::Finished(code, _)) => {
                                        p.done = true;
                                        p.exit = code;
                                        break;
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                        Kind::Pty { child, output, .. } => {
                            let drained = match output.try_recv() {
                                Ok(bytes) => {
                                    p.pending.extend(bytes);
                                    false
                                }
                                Err(mpsc::TryRecvError::Disconnected) => true,
                                Err(_) => false,
                            };
                            if let Some(child) = child {
                                if let Ok(Some(status)) = child.try_wait() {
                                    p.exit = Some(status.exit_code() as i32);
                                }
                            }
                            p.done = drained && p.exit.is_some();
                        }
                    }
                }
                let bytes = p.pending.drain(..p.pending.len().min(8192)).collect();
                let done = p.done && p.pending.is_empty();
                let exit = p.exit;
                if done {
                    self.processes.remove(id);
                }
                Ok(Response::Process { bytes, done, exit })
            })()),
            Request::ProcessInput { id, bytes } => Some((|| {
                self.authorized(connection, self.epoch)?;
                if bytes.len() > 8192 {
                    return Err(Error::Limit);
                }
                let p = self.processes.get(id).ok_or(Error::Missing)?;
                if let Kind::Pty { input, .. } = &p.kind {
                    input.try_send(bytes.clone()).map_err(|_| Error::Busy)?;
                    Ok(Response::Pong {})
                } else {
                    Err(Error::Unsupported)
                }
            })()),
            Request::ProcessResize { id, cols, rows } => Some((|| {
                self.authorized(connection, self.epoch)?;
                if *cols == 0 || *rows == 0 || *cols > 512 || *rows > 256 {
                    return Err(Error::Limit);
                }
                let p = self.processes.get(id).ok_or(Error::Missing)?;
                if let Kind::Pty { master, .. } = &p.kind {
                    master
                        .resize(PtySize {
                            cols: *cols,
                            rows: *rows,
                            pixel_width: 0,
                            pixel_height: 0,
                        })
                        .map_err(|_| Error::Unavailable)?;
                    Ok(Response::Pong {})
                } else {
                    Err(Error::Unsupported)
                }
            })()),
            Request::ProcessStop { id } => Some((|| {
                self.authorized(connection, self.epoch)?;
                if let Some(mut process) = self.processes.remove(id) {
                    process.stop();
                }
                Ok(Response::Pong {})
            })()),
            _ => None,
        }
    }
}
fn execute(
    files: Files,
    mut call: Call,
    open: HashMap<PathBuf, String>,
    cancel: Arc<AtomicBool>,
    git_lock: Arc<Mutex<()>>,
) -> std::result::Result<serde_json::Value, String> {
    match call {
        Call::File { path, query } => files.call(&path, query, &cancel),
        Call::Search {
            roots,
            filter,
            query,
            replacement,
            options,
            open: client_open,
        } => {
            let mut merged = HashMap::new();
            for (path, text) in client_open {
                let actual = files.actual(&path).map_err(|e| e.to_string())?;
                let text = open.get(&actual).cloned().unwrap_or(text);
                merged.insert(actual, text);
            }
            serde_json::to_value(files.search(
                &roots,
                &filter,
                &query,
                replacement.as_deref(),
                &options,
                &merged,
                &cancel,
            )?)
            .map_err(|_| "Cannot encode search results.".into())
        }
        Call::Replace(plan) => {
            let path = Files::relative(&plan.path).map_err(|e| e.to_string())?;
            let (_, _, _, hash) =
                host::read_encoded(&files.root, &path).map_err(|e| e.to_string())?;
            if hash != plan.hash {
                return Err("The file changed after preview; edits were preserved.".into());
            }
            let bytes = hedit_core::text_encoding::encode(&plan.after, &plan.encoding, plan.bom)?;
            host::atomic_save(&files.root, &path, &bytes, Some(&hash))
                .map_err(|e| e.to_string())?;
            use sha2::Digest;
            serde_json::to_value(hedit_core::search::Restore {
                path: files.actual(&plan.path).map_err(|e| e.to_string())?,
                before: plan.before,
                written: sha2::Sha256::digest(&bytes).into(),
            })
            .map_err(|_| "Cannot encode replacement result.".into())
        }
        Call::Restore(record) => {
            let path = Files::relative(&record.path).map_err(|e| e.to_string())?;
            let (_, encoding, bom, hash) =
                host::read_encoded(&files.root, &path).map_err(|e| e.to_string())?;
            if hash != record.written {
                return Err("The file changed after replacement.".into());
            }
            let bytes = hedit_core::text_encoding::encode(&record.before, &encoding, bom)?;
            host::atomic_save(&files.root, &path, &bytes, Some(&hash))
                .map_err(|e| e.to_string())?;
            Ok(serde_json::Value::Null)
        }
        Call::Git { .. } => {
            let _guard = git_lock
                .lock()
                .map_err(|_| "Repository worker unavailable.")?;
            call.map_paths(|path| files.actual(path).map_err(|e| e.to_string()))?;
            if let Call::Git { path, query } = &mut call {
                if !matches!(query, GitQuery::Clone(_)) {
                    let relative = path
                        .strip_prefix(&files.path)
                        .map_err(|_| "Repository outside workspace.")?;
                    let _ = files.directory(relative).map_err(|e| e.to_string())?;
                }
                if let GitQuery::Execute(
                    hedit_core::git::Operation::Discard { backup, .. }
                    | hedit_core::git::Operation::Resolve { backup, .. },
                ) = query
                {
                    *backup = files.state.join("git-backups");
                }
            }
            call.local(cancel)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn near_limit_response_reserves_space_for_its_reply_envelope() {
        let mut runtime = Runtime::default();
        let response = Response::Value {
            value: serde_json::json!("x".repeat(protocol::MAX_FRAME - 128)),
        };
        assert!(protocol::encode(&response).is_ok());
        let response = runtime.reply(1, Ok(response)).unwrap();
        assert!(matches!(response, Response::Transfer { .. }));
        let reply = protocol::Reply {
            version: protocol::VERSION,
            id: u64::MAX,
            result: Ok(response),
        };
        assert!(protocol::encode(&reply).is_ok());
    }
}
