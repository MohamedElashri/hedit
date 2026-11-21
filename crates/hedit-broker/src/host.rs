//! Bounded host threads own blocking filesystem work and callback supervision.
//! UI handles only exchange immutable results and exact preview decisions.
use crate::{
    cancel::Cancel,
    plan::Plan,
    session::{Config, Effect, Session},
    Handle,
};
use hedit_plugin::{broker::FailureCode, sdk, service};
use std::{
    fs::File,
    io,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};
static ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);
/// Resolve the exact installed source, scope and policy generation under a
/// shared disclosure lock. The returned file retains that lock. Cancellation
/// must precede exclusive policy mutation. Never build this callback from Lua.
pub type Authority = dyn Fn(&AtomicBool) -> io::Result<File> + Send + Sync;
/// The editor validates and freezes affected buffers on this host's I/O thread.
/// The returned owner performs reconciliation and unfreezes on Drop, including
/// errors after a partial journal effect. It is never transferred to the UI.
pub trait WorkspaceCommit: Send {
    fn applied(&mut self) {}
}
pub type PrepareCommit =
    dyn Fn(&Arc<Plan>, &AtomicBool) -> io::Result<Box<dyn WorkspaceCommit>> + Send + Sync;
pub enum Event {
    Review(Arc<Plan>),
    Finished {
        transaction: Option<sdk::Transaction>,
        result: Option<Box<service::ResultSet>>,
        cache: sdk::Storage,
    },
    Failed(FailureCode),
}
pub struct Decision {
    pub plan: Handle,
    pub accept: bool,
}
struct Mailbox {
    value: Mutex<Option<Event>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl Mailbox {
    fn publish(&self, event: Event) {
        let previous = self
            .value
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .replace(event);
        drop(previous);
        // Wake callbacks must not turn a host panic into a second panic during
        // Completion::drop. The mailbox is already published and unlocked.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.wake)()));
    }
}
/// Dropping/stopping never joins a thread or waits on filesystem/worker I/O.
/// Before accepting a preview, the UI must verify and freeze affected clean
/// buffers. It must release that freeze on every terminal event or cancellation.
pub struct Host {
    mailbox: Arc<Mailbox>,
    decisions: tokio::sync::mpsc::Sender<Decision>,
    cancel: Arc<Cancel>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Host {
    /// `setup` performs resource/grant resolution on the new bounded I/O thread.
    /// Final edits/presentation still require fresh host authorization and
    /// document/revision checks when the editor consumes `Finished`.
    pub fn start(
        setup: impl FnOnce(Arc<Cancel>) -> io::Result<Config> + Send + 'static,
        authority: Arc<Authority>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> io::Result<Self> {
        Self::start_inner(setup, authority, None, wake)
    }
    pub fn start_scoped(
        setup: impl FnOnce(Arc<Cancel>) -> io::Result<Config> + Send + 'static,
        authority: Arc<Authority>,
        prepare: Arc<PrepareCommit>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> io::Result<Self> {
        Self::start_inner(setup, authority, Some(prepare), wake)
    }
    fn start_inner(
        setup: impl FnOnce(Arc<Cancel>) -> io::Result<Config> + Send + 'static,
        authority: Arc<Authority>,
        prepare: Option<Arc<PrepareCommit>>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> io::Result<Self> {
        let permit = ADMISSION.try_acquire().map_err(|_| {
            io::Error::new(io::ErrorKind::WouldBlock, "Extension host limit reached.")
        })?;
        let cancel = Arc::new(Cancel::default());
        let mailbox = Arc::new(Mailbox {
            value: Mutex::new(None),
            wake,
        });
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let thread_cancel = cancel.clone();
        let thread_mailbox = mailbox.clone();
        let thread = std::thread::Builder::new()
            .name("hedit-capability-host".into())
            .spawn(move || {
                let _permit = permit;
                struct Completion {
                    mailbox: Arc<Mailbox>,
                    cancel: Arc<Cancel>,
                    finished: bool,
                }
                impl Drop for Completion {
                    fn drop(&mut self) {
                        if !self.finished {
                            self.cancel.cancel();
                            self.mailbox.publish(Event::Failed(FailureCode::Failed));
                        }
                    }
                }
                let mut completion = Completion {
                    mailbox: thread_mailbox.clone(),
                    cancel: thread_cancel.clone(),
                    finished: false,
                };
                let result = (|| {
                    if thread_cancel.is_cancelled() {
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "Extension host cancelled.",
                        ));
                    }
                    let config = setup(thread_cancel.clone())?;
                    if !Arc::ptr_eq(&config.cancel, &thread_cancel) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "Host cancellation identity changed.",
                        ));
                    }
                    let mut session = Session::new(config)?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    runtime.block_on(run(
                        &mut session,
                        &thread_cancel,
                        authority,
                        prepare,
                        receiver,
                        &thread_mailbox,
                    ))
                })();
                let event = match result {
                    Ok(event) => event,
                    Err(error) => Event::Failed(code(&error)),
                };
                thread_mailbox.publish(event);
                completion.finished = true;
            })?;
        Ok(Self {
            mailbox,
            decisions: sender,
            cancel,
            thread: Some(thread),
        })
    }
    pub fn try_event(&self) -> Option<Event> {
        let event = self.mailbox.value.try_lock().ok()?.take()?;
        if self.cancel.is_cancelled() && matches!(event, Event::Review(_)) {
            return None;
        }
        Some(event)
    }
    pub fn decide(&self, decision: Decision) -> io::Result<()> {
        if self.cancel.is_cancelled() {
            return Err(cancelled());
        }
        self.decisions.try_send(decision).map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "The extension host is not awaiting this decision.",
            )
        })
    }
    pub fn stop(&self) {
        self.cancel.cancel();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
        self.thread.take(); // Detach; native cleanup stays off the UI thread.
    }
}
fn code(error: &io::Error) -> FailureCode {
    match error.kind() {
        io::ErrorKind::Interrupted => FailureCode::Cancelled,
        io::ErrorKind::PermissionDenied => FailureCode::Denied,
        io::ErrorKind::AlreadyExists | io::ErrorKind::NotFound => FailureCode::Changed,
        io::ErrorKind::TimedOut | io::ErrorKind::OutOfMemory | io::ErrorKind::WouldBlock => {
            FailureCode::Limit
        }
        io::ErrorKind::Unsupported => FailureCode::Unavailable,
        _ => FailureCode::Failed,
    }
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "Extension host cancelled.")
}
fn worker_error(error: hedit_plugin::Error) -> io::Error {
    use hedit_plugin::Error;
    let kind = match error {
        Error::Cancelled | Error::Stopped => io::ErrorKind::Interrupted,
        Error::Sandbox | Error::Unavailable => io::ErrorKind::Unsupported,
        Error::Deadline => io::ErrorKind::TimedOut,
        Error::ServiceBusy => io::ErrorKind::WouldBlock,
        error if error.is_limit() => io::ErrorKind::OutOfMemory,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error)
}
async fn run(
    session: &mut Session,
    cancel: &Arc<Cancel>,
    authority: Arc<Authority>,
    prepare: Option<Arc<PrepareCommit>>,
    mut decisions: tokio::sync::mpsc::Receiver<Decision>,
    mailbox: &Mailbox,
) -> io::Result<Event> {
    loop {
        let input = session.begin_callback()?;
        let completed = Arc::new(tokio::sync::Notify::new());
        let wake = completed.clone();
        let authorize = authority.clone();
        let session_cancel = cancel.clone();
        let mut worker = hedit_plugin::start_broker_checked(
            input,
            move |flag| {
                if flag.load(std::sync::atomic::Ordering::Acquire) || session_cancel.is_cancelled()
                {
                    return Err(hedit_plugin::Error::Cancelled);
                }
                authorize(flag).map_err(|_| hedit_plugin::Error::Cancelled)
            },
            move || {
                wake.notify_one();
            },
        )
        .map_err(worker_error)?;
        let output = loop {
            if let Some(result) = worker.try_result() {
                break result.map_err(|failure| worker_error(failure.kind))?;
            }
            tokio::select! { biased; _ = cancel.cancelled() => return Err(cancelled()), _ = completed.notified() => {} }
        };
        drop(worker);
        match session.accept(output, || authority(cancel.flag())).await? {
            Effect::Continue => {}
            Effect::Finished {
                transaction,
                result,
                cache,
            } => {
                return Ok(Event::Finished {
                    transaction,
                    result,
                    cache,
                })
            }
            Effect::Review(plan) => {
                let plan = Arc::new(plan);
                mailbox.publish(Event::Review(plan.clone()));
                let decision = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Err(cancelled()),
                    result = tokio::time::timeout(Duration::from_secs(120),decisions.recv()) => result.map_err(|_|io::Error::new(io::ErrorKind::TimedOut,"Workspace preview expired."))?.ok_or_else(cancelled)?,
                };
                if decision.plan != plan.id() || !plan.live() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Stale workspace preview decision.",
                    ));
                }
                if decision.accept {
                    let _active = session.host_work()?;
                    let started = std::time::Instant::now();
                    let mut buffers = prepare
                        .as_ref()
                        .map(|prepare| prepare(&plan, cancel.flag()))
                        .transpose()?;
                    session.charge_host_work(started.elapsed())?;
                    let journal =
                        session.prepare_review_in_slot(&plan, || authority(cancel.flag()))?;
                    session.apply_review_in_slot(&plan, &journal, || authority(cancel.flag()))?;
                    if let Some(buffers) = &mut buffers {
                        buffers.applied();
                    }
                } else {
                    session.discard_review(&plan)?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
