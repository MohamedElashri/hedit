//! One persistent native I/O thread; UI drops only signal cancellation.
mod peer;
use super::{Connection, Method, Transport};
use crate::{
    cancel::Cancel,
    tool::{Run, Service},
};
use hedit_plugin::{sdk, service::ResultSet, tool::Progress};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant},
};

struct Native {
    service: Service,
    close: Arc<Cancel>,
    scope: Arc<Cancel>,
    request: Option<Arc<Cancel>>,
}
impl Native {
    fn check(&self) -> io::Result<()> {
        if self.close.is_cancelled()
            || self.scope.is_cancelled()
            || self
                .request
                .as_ref()
                .is_some_and(|cancel| cancel.is_cancelled())
        {
            return Err(cancelled());
        }
        Ok(())
    }
}
impl Transport for Native {
    fn poll(
        &mut self,
        input: &[u8],
        output: &mut [u8; 4096],
        _: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Progress> {
        self.check()?;
        self.service
            .poll(input, output, self.close.flag(), deadline)
    }
    fn wait(&self, input_pending: bool) -> io::Result<()> {
        self.check()?;
        self.service.wait(input_pending)
    }
}
struct Query {
    snapshot: sdk::Snapshot,
    method: Method,
    position: usize,
    may_edit: bool,
    cancel: Arc<Cancel>,
    deadline: Instant,
    reply: SyncSender<io::Result<ResultSet>>,
}
pub struct Owner {
    requests: SyncSender<Query>,
    close: Arc<Cancel>,
    scope: Arc<Cancel>,
    alive: Arc<AtomicBool>,
    document: String,
}
fn cancelled() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "Scoped language-server owner cancelled.",
    )
}
fn failed() -> io::Error {
    io::Error::other("Scoped language-server owner stopped; restart it explicitly.")
}
fn timeout() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "Scoped language-server owner timed out.",
    )
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.close.cancel();
    }
}
impl Owner {
    /// The caller has verified a source-bound LSP grant and holds disclosure
    /// authority throughout startup. `scope` must be registered with the host
    /// policy watcher and cancelled when its document/workspace owner closes.
    pub fn start(
        run: Run,
        relative: String,
        snapshot: sdk::Snapshot,
        workspace: bool,
        scope: Arc<Cancel>,
        deadline: Instant,
    ) -> io::Result<Self> {
        let deadline = deadline.min(Instant::now() + Duration::from_secs(10));
        snapshot.validate().map_err(|_| failed())?;
        let uri = run.document_uri(&relative, snapshot.text.len())?;
        let root = workspace
            .then(|| hedit_core::language_server::text::uri(run.snapshot_directory()))
            .transpose()
            .map_err(|_| failed())?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let (ready, incoming) = mpsc::sync_channel(1);
        let owner = Self {
            requests: sender,
            close: Arc::new(Cancel::default()),
            scope,
            alive: Arc::new(AtomicBool::new(true)),
            document: snapshot.document.clone(),
        };
        let close = owner.close.clone();
        let scope = owner.scope.clone();
        let alive = owner.alive.clone();
        std::thread::Builder::new()
            .name("hedit-plugin-lsp".into())
            .spawn(move || {
                struct End(Arc<AtomicBool>);
                impl Drop for End {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release);
                    }
                }
                let _end = End(alive);
                let initialize = || {
                    if scope.is_cancelled() || close.is_cancelled() {
                        return Err(cancelled());
                    }
                    let service = run.service(scope.flag(), deadline)?;
                    let native = Native {
                        service,
                        close: close.clone(),
                        scope: scope.clone(),
                        request: None,
                    };
                    peer::Peer::start(
                        Connection::new(native),
                        snapshot,
                        uri,
                        root,
                        close.flag(),
                        deadline,
                    )
                };
                let mut peer = match initialize() {
                    Ok(peer) => {
                        if ready.send(Ok(())).is_err() {
                            return;
                        }
                        peer
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                loop {
                    if close.is_cancelled() || scope.is_cancelled() {
                        return;
                    }
                    match receiver.recv_timeout(Duration::from_millis(50)) {
                        Ok(query) => {
                            let query: Query = query;
                            if peer
                                .connection
                                .transport_mut()
                                .map(|native| native.request = Some(query.cancel.clone()))
                                .is_err()
                            {
                                let _ = query.reply.send(Err(failed()));
                                return;
                            }
                            let result = peer.request(
                                query.snapshot,
                                query.method,
                                query.position,
                                query.may_edit,
                                close.flag(),
                                query.deadline,
                            );
                            let stopped = result.as_ref().is_err_and(|error| {
                                !matches!(
                                    error.kind(),
                                    io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
                                )
                            });
                            if let Ok(native) = peer.connection.transport_mut() {
                                native.request = None;
                            }
                            let _ = query.reply.send(result);
                            if stopped {
                                return;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if peer
                                .idle(close.flag(), Instant::now() + Duration::from_secs(1))
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            })?;
        wait(&incoming, &owner.close, &owner.scope, None, deadline)??;
        Ok(owner)
    }
    pub fn running(&self) -> bool {
        self.alive.load(Ordering::Acquire)
            && !self.close.is_cancelled()
            && !self.scope.is_cancelled()
    }
    /// True only after the I/O thread has dropped/reaped the native transport
    /// and retired its private bundle. UI code must never wait for this flag.
    pub fn settled(&self) -> bool {
        !self.alive.load(Ordering::Acquire)
    }
    pub fn request(
        &self,
        snapshot: &sdk::Snapshot,
        method: Method,
        position: usize,
        may_edit: bool,
        cancel: Arc<Cancel>,
        deadline: Instant,
    ) -> io::Result<ResultSet> {
        if !self.running() {
            return Err(failed());
        }
        let deadline = deadline.min(Instant::now() + Duration::from_secs(10));
        snapshot.validate().map_err(|_| failed())?;
        if snapshot.document != self.document || !sdk::boundary(&snapshot.text, position) {
            return Err(failed());
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.requests
            .try_send(Query {
                snapshot: snapshot.clone(),
                method,
                position,
                may_edit,
                cancel: cancel.clone(),
                deadline,
                reply,
            })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Scoped language-server owner is busy.",
                )
            })?;
        match wait(&receiver, &self.close, &self.scope, Some(&cancel), deadline) {
            Ok(result) => {
                if result.as_ref().is_err_and(|error| {
                    !matches!(
                        error.kind(),
                        io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
                    )
                }) {
                    self.close.cancel();
                }
                result
            }
            Err(error) => {
                self.close.cancel();
                Err(error)
            }
        }
    }
}
fn wait<T>(
    receiver: &Receiver<T>,
    close: &Cancel,
    scope: &Cancel,
    request: Option<&Cancel>,
    deadline: Instant,
) -> io::Result<T> {
    loop {
        if close.is_cancelled() || scope.is_cancelled() || request.is_some_and(Cancel::is_cancelled)
        {
            return Err(cancelled());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(timeout)?;
        match receiver.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(value) => {
                if close.is_cancelled()
                    || scope.is_cancelled()
                    || request.is_some_and(Cancel::is_cancelled)
                {
                    return Err(cancelled());
                }
                if Instant::now() >= deadline {
                    return Err(timeout());
                }
                return Ok(value);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(failed()),
        }
    }
}
