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
    lifetime: Option<hedit_core::lifetime::Lease>,
}
impl Native {
    fn check(&self) -> io::Result<()> {
        if self
            .lifetime
            .as_ref()
            .is_some_and(|lifetime| !lifetime.current())
            || self.close.is_cancelled()
            || self.scope.is_cancelled()
        {
            self.close.cancel();
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
    lifetime: Option<hedit_core::lifetime::Lease>,
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
        lifetime: Option<hedit_core::lifetime::Lease>,
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
            lifetime,
        };
        let close = owner.close.clone();
        let scope = owner.scope.clone();
        let alive = owner.alive.clone();
        let lifetime = owner.lifetime.clone();
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
                    if scope.is_cancelled()
                        || close.is_cancelled()
                        || lifetime
                            .as_ref()
                            .is_some_and(|lifetime| !lifetime.current())
                    {
                        return Err(cancelled());
                    }
                    let service = run.service(close.flag(), deadline)?;
                    let native = Native {
                        service,
                        close: close.clone(),
                        scope: scope.clone(),
                        lifetime: lifetime.clone(),
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
                    if close.is_cancelled()
                        || scope.is_cancelled()
                        || lifetime
                            .as_ref()
                            .is_some_and(|lifetime| !lifetime.current())
                    {
                        return;
                    }
                    match receiver.recv_timeout(Duration::from_millis(50)) {
                        Ok(query) => {
                            let query: Query = query;
                            let result = peer.request(
                                query.snapshot,
                                query.method,
                                query.position,
                                query.may_edit,
                                (close.flag(), query.cancel.flag()),
                                query.deadline,
                            );
                            let stopped = result.as_ref().is_err_and(|error| {
                                !matches!(
                                    error.kind(),
                                    io::ErrorKind::Unsupported
                                        | io::ErrorKind::PermissionDenied
                                        | io::ErrorKind::Interrupted
                                )
                            });
                            let _ = query.reply.send(result);
                            if stopped || !peer.connection.open() {
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
        wait(
            &incoming,
            &owner.close,
            &owner.scope,
            None,
            deadline,
            owner.lifetime.as_ref(),
        )??;
        Ok(owner)
    }
    pub fn running(&self) -> bool {
        self.alive.load(Ordering::Acquire)
            && !self.close.is_cancelled()
            && !self.scope.is_cancelled()
            && self
                .lifetime
                .as_ref()
                .is_none_or(hedit_core::lifetime::Lease::current)
    }
    /// True after the I/O thread has dropped/reaped the native transport and
    /// released its bundle lease for cleanup. UI code must never wait here.
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
        // Keep the caller's disclosure guard alive until the I/O thread has
        // completed any partial frame and acknowledged the superseded ID.
        match wait(
            &receiver,
            &self.close,
            &self.scope,
            None,
            deadline,
            self.lifetime.as_ref(),
        ) {
            Ok(result) => {
                if cancel.is_cancelled() {
                    return Err(cancelled());
                }
                if result.as_ref().is_err_and(|error| {
                    !matches!(
                        error.kind(),
                        io::ErrorKind::Unsupported
                            | io::ErrorKind::PermissionDenied
                            | io::ErrorKind::Interrupted
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
    lifetime: Option<&hedit_core::lifetime::Lease>,
) -> io::Result<T> {
    loop {
        if lifetime.is_some_and(|lifetime| !lifetime.current())
            || close.is_cancelled()
            || scope.is_cancelled()
            || request.is_some_and(Cancel::is_cancelled)
        {
            return Err(cancelled());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(timeout)?;
        match receiver.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(value) => {
                if lifetime.is_some_and(|lifetime| !lifetime.current())
                    || close.is_cancelled()
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
