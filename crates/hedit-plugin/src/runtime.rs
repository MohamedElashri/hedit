use crate::{options::Value, Error};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub(crate) mod engine;
mod protocol;
#[cfg(unix)]
#[path = "runtime/unix.rs"]
mod supervisor;
#[cfg(windows)]
#[path = "runtime/windows.rs"]
mod supervisor;

#[cfg(not(any(unix, windows)))]
mod supervisor {
    use crate::Error;
    use std::{
        fs::File,
        path::{Path, PathBuf},
        sync::atomic::AtomicBool,
        time::Duration,
    };

    pub(super) fn executable() -> Result<PathBuf, Error> {
        Err(Error::Sandbox)
    }

    pub(super) fn supervise_service(
        _: &Path,
        _: Vec<u8>,
        _: &AtomicBool,
        _: Duration,
        _: Option<File>,
        _: &crate::telemetry::Recorder,
    ) -> Result<Vec<u8>, Error> {
        Err(Error::Unavailable)
    }
    pub(super) fn supervise_profiled(
        _: &Path,
        _: Vec<u8>,
        _: &AtomicBool,
        _: Duration,
        _: Option<File>,
        _: &crate::telemetry::Recorder,
    ) -> Result<Vec<u8>, Error> {
        Err(Error::Unavailable)
    }

    pub(super) fn supervise(
        _: &Path,
        _: Vec<u8>,
        _: &AtomicBool,
        _: Duration,
        _: Option<File>,
    ) -> Result<Vec<u8>, Error> {
        Err(Error::Sandbox)
    }
}

pub const SUPPORTED: bool = crate::sandbox::SUPPORTED;
pub const MAX_SELECTION_BYTES: usize = 64 * 1024;
pub const MAX_SELECTIONS: usize = 16;
pub const MAX_SOURCE_BYTES: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub source: String,
    pub selections: Vec<String>,
    pub language: String,
    pub settings: BTreeMap<String, Value>,
}

pub const HELPER_ARG: &str = "--hedit-plugin-worker";
pub const SERVICE_HELPER_ARG: &str = "--hedit-plugin-service-worker";

pub type RunningPlugin = Running<Vec<String>>;
pub type RunningBroker = Running<crate::broker::Output, crate::sdk::Failure>;
pub type RunningSdk = Running<crate::sdk::Transaction, crate::sdk::Failure>;

pub struct Running<T, E = Error> {
    receiver: mpsc::Receiver<Result<T, E>>,
    cancelled: Arc<AtomicBool>,
    trace: crate::telemetry::Recorder,
}

impl<T, E: From<Error>> Running<T, E> {
    pub fn timings(&self) -> crate::telemetry::Recorder {
        self.trace.clone()
    }
    pub fn try_result(&mut self) -> Option<Result<T, E>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::Stopped.into())),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn from_test_result(result: Result<T, E>) -> Self {
        let (sender, receiver) = mpsc::channel();
        sender.send(result).unwrap();
        Self {
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
            trace: Default::default(),
        }
    }
}

impl<T, E> Drop for Running<T, E> {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// Queue the result before waking the host event loop.
pub fn start(
    input: Input,
    approval: File,
    wake: impl FnOnce() + Send + 'static,
) -> Result<RunningPlugin, Error> {
    launch(approval, wake, move |approval, cancelled, _trace| {
        run(input, Some(approval), cancelled, false)
    })
}

pub fn start_sdk(
    input: crate::sdk::Input,
    approval: File,
    wake: impl FnOnce() + Send + 'static,
) -> Result<RunningSdk, Error> {
    launch(approval, wake, move |approval, cancelled, trace| {
        engine::run_sdk_profiled(input, Some(approval), cancelled, false, trace)
    })
}

struct ServiceBudget {
    active: std::sync::atomic::AtomicUsize,
    background: std::sync::atomic::AtomicUsize,
}
impl ServiceBudget {
    const fn new() -> Self {
        Self {
            active: std::sync::atomic::AtomicUsize::new(0),
            background: std::sync::atomic::AtomicUsize::new(0),
        }
    }
    fn available(&self, background: bool) -> bool {
        self.active.load(Ordering::Acquire) < crate::service::MAX_ACTIVE
            && (!background
                || self.background.load(Ordering::Acquire) < crate::service::MAX_ACTIVE - 1)
    }
    fn reserve(&'static self, background: bool) -> Result<ServicePermit, Error> {
        fn acquire(counter: &std::sync::atomic::AtomicUsize, maximum: usize) -> Result<(), Error> {
            let mut old = counter.load(Ordering::Acquire);
            loop {
                if old >= maximum {
                    return Err(Error::ServiceBusy);
                }
                match counter.compare_exchange_weak(
                    old,
                    old + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return Ok(()),
                    Err(actual) => old = actual,
                }
            }
        }
        if background {
            acquire(&self.background, crate::service::MAX_ACTIVE - 1)?;
        }
        if let Err(error) = acquire(&self.active, crate::service::MAX_ACTIVE) {
            if background {
                self.background.fetch_sub(1, Ordering::AcqRel);
            }
            return Err(error);
        }
        Ok(ServicePermit {
            budget: self,
            background,
        })
    }
}
static SERVICES: ServiceBudget = ServiceBudget::new();
pub fn service_available() -> bool {
    SERVICES.available(true)
}
pub fn broker_available(background: bool) -> bool {
    SERVICES.available(background)
}
pub struct ServiceAdmission {
    _permit: ServicePermit,
}
pub fn service_admission() -> Result<ServiceAdmission, Error> {
    Ok(ServiceAdmission {
        _permit: SERVICES.reserve(true)?,
    })
}
struct ServicePermit {
    budget: &'static ServiceBudget,
    background: bool,
}
impl Drop for ServicePermit {
    fn drop(&mut self) {
        self.budget.active.fetch_sub(1, Ordering::AcqRel);
        if self.background {
            self.budget.background.fetch_sub(1, Ordering::AcqRel);
        }
    }
}
pub type RunningService = Running<crate::service::Output, crate::sdk::Failure>;
pub fn start_service(
    input: crate::service::Input,
    approval: File,
    wake: impl FnOnce() + Send + 'static,
) -> Result<RunningService, Error> {
    start_service_checked(input, move |_| Ok(approval), wake)
}
pub fn start_service_checked(
    input: crate::service::Input,
    authorize: impl FnOnce(&AtomicBool) -> Result<File, Error> + Send + 'static,
    wake: impl FnOnce() + Send + 'static,
) -> Result<RunningService, Error> {
    let permit = SERVICES.reserve(true)?;
    spawn(wake, move |cancelled, trace| {
        let _permit = permit;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled.into());
        }
        let approval = authorize(cancelled)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled.into());
        }
        engine::run_service(input, Some(approval), cancelled, trace)
    })
}
pub fn run_service_fixture(
    input: crate::service::Input,
) -> Result<crate::service::Output, crate::sdk::Failure> {
    let _permit = SERVICES.reserve(true)?;
    engine::run_service(input, None, &AtomicBool::new(false), &Default::default())
}

pub fn start_broker_checked(
    input: crate::broker::Input,
    authorize: impl FnOnce(&AtomicBool) -> Result<File, Error> + Send + 'static,
    wake: impl FnOnce() + Send + 'static,
) -> Result<RunningBroker, Error> {
    start_broker_prioritized(input, false, authorize, wake)
}
/// `background` comes from the trusted broker declaration, never the Lua reply.
/// Isolation is identical; background callbacks leave one worker slot available.
pub fn start_broker_prioritized(
    input: crate::broker::Input,
    background: bool,
    authorize: impl FnOnce(&AtomicBool) -> Result<File, Error> + Send + 'static,
    wake: impl FnOnce() + Send + 'static,
) -> Result<RunningBroker, Error> {
    let permit = SERVICES.reserve(background)?;
    spawn(wake, move |cancelled, trace| {
        let _permit = permit;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled.into());
        }
        let approval = authorize(cancelled)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled.into());
        }
        engine::run_broker(input, Some(approval), cancelled, trace)
    })
}
pub fn run_broker_fixture(
    input: crate::broker::Input,
) -> Result<crate::broker::Output, crate::sdk::Failure> {
    run_broker_fixture_traced(input, &Default::default())
}
/// Offline author fixture: fresh confined worker, no resource effects or grants.
pub fn run_broker_fixture_traced(
    input: crate::broker::Input,
    trace: &crate::telemetry::Recorder,
) -> Result<crate::broker::Output, crate::sdk::Failure> {
    let _permit = SERVICES.reserve(false)?;
    engine::run_broker(input, None, &AtomicBool::new(false), trace)
}

fn launch<T: Send + 'static, E: Send + 'static>(
    approval: File,
    wake: impl FnOnce() + Send + 'static,
    run: impl FnOnce(File, &AtomicBool, &crate::telemetry::Recorder) -> Result<T, E> + Send + 'static,
) -> Result<Running<T, E>, Error> {
    spawn(wake, move |cancelled, trace| {
        run(approval, cancelled, trace)
    })
}
fn spawn<T: Send + 'static, E: Send + 'static>(
    wake: impl FnOnce() + Send + 'static,
    run: impl FnOnce(&AtomicBool, &crate::telemetry::Recorder) -> Result<T, E> + Send + 'static,
) -> Result<Running<T, E>, Error> {
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = cancelled.clone();
    let trace = crate::telemetry::Recorder::default();
    let thread_trace = trace.clone();
    std::thread::Builder::new()
        .name("hedit-lua-worker".into())
        .spawn(move || {
            struct Completion<T, F: FnOnce()> {
                sender: Option<mpsc::Sender<T>>,
                wake: Option<F>,
            }
            impl<T, F: FnOnce()> Drop for Completion<T, F> {
                fn drop(&mut self) {
                    self.sender.take();
                    if let Some(wake) = self.wake.take() {
                        wake();
                    }
                }
            }
            let completion = Completion {
                sender: Some(sender),
                wake: Some(wake),
            };
            let result = run(&thread_cancelled, &thread_trace);
            let _ = completion.sender.as_ref().unwrap().send(result);
        })
        .map_err(|_| Error::Startup)?;
    Ok(Running {
        receiver,
        cancelled,
        trace,
    })
}

fn run_sdk(
    input: crate::sdk::Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<crate::sdk::Transaction, crate::sdk::Failure> {
    if !SUPPORTED {
        return Err(Error::Unavailable.into());
    }
    engine::run_sdk(input, approval, cancelled, save)
}

pub fn run_sdk_fixture(
    input: crate::sdk::Input,
) -> Result<crate::sdk::Prepared, crate::sdk::Failure> {
    run_sdk_fixture_traced(input, &Default::default())
}
pub fn run_sdk_fixture_traced(
    input: crate::sdk::Input,
    trace: &crate::telemetry::Recorder,
) -> Result<crate::sdk::Prepared, crate::sdk::Failure> {
    let snapshot = input.snapshot.clone();
    Ok(
        engine::run_sdk_profiled(input, None, &AtomicBool::new(false), false, trace)?
            .prepare(&snapshot)?,
    )
}

pub fn run_sdk_save(
    input: crate::sdk::Input,
    approval: File,
) -> Result<String, crate::sdk::Failure> {
    let snapshot = input.snapshot.clone();
    Ok(
        run_sdk(input, Some(approval), &AtomicBool::new(false), true)?
            .prepare(&snapshot)?
            .text,
    )
}

pub fn run_fixture(input: Input) -> Result<String, Error> {
    run_one(input, None, false)
}

pub fn run_save(input: Input, approval: File) -> Result<String, Error> {
    run_one(input, Some(approval), true)
}

fn run_one(input: Input, approval: Option<File>, save: bool) -> Result<String, Error> {
    let [replacement] = run(input, approval, &AtomicBool::new(false), save)?
        .try_into()
        .map_err(|_| Error::EditCount)?;
    Ok(replacement)
}

fn run(
    input: Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<Vec<String>, Error> {
    if !SUPPORTED {
        return Err(Error::Unavailable);
    }
    engine::run(input, approval, cancelled, save)
}

pub fn run_if_requested(args: &[OsString]) -> Option<bool> {
    let service = args == [OsStr::new(SERVICE_HELPER_ARG)];
    if args != [OsStr::new(HELPER_ARG)] && !service {
        return None;
    }
    if !SUPPORTED {
        return Some(false);
    }
    // Install the OS boundary before the worker touches its request pipes.
    let isolated = if service {
        crate::sandbox::enter_service().is_ok()
    } else {
        crate::sandbox::enter().is_ok()
    };
    Some(if service {
        engine::run_service_helper(isolated)
    } else {
        engine::run_helper(isolated)
    })
}

#[cfg(feature = "fuzzing")]
pub(crate) fn fuzz_protocol(bytes: &[u8]) {
    if bytes.len() > crate::broker::MAX_MESSAGE_BYTES {
        return;
    }
    let _ = protocol::decode_invocation(bytes);
    let _ = crate::broker::json::decode(bytes);
    let _ = protocol::decode_edits(bytes, 1);
    let snapshot = crate::sdk::Snapshot {
        document: "0123456789abcdef0123456789abcdef".into(),
        revision: "42".into(),
        language: "markdown".into(),
        text: "café / 東京\r\n🦀".into(),
        selections: vec![crate::sdk::Selection { anchor: 0, head: 0 }],
        crlf: true,
    };
    if let Ok(transaction) = serde_json::from_slice::<crate::sdk::Transaction>(bytes) {
        let _ = transaction.prepare(&snapshot);
    }
    let input = crate::sdk::Input {
        sdk_version: 1,
        source: "return function(ctx) return hedit.transaction({}) end".into(),
        modules: Default::default(),
        resources: Default::default(),
        snapshot,
        settings: Default::default(),
        arguments: Default::default(),
        storage: None,
    };
    let _ = engine::decode_sdk_reply(&input, bytes);
    let input = crate::service::Input {
        protocol_version: crate::service::VERSION,
        request: crate::service::Request {
            id: "f".repeat(32),
            kind: crate::service::Kind::Diagnostics,
            trigger: crate::service::Trigger::Change,
            position: 0,
            action: None,
            choice: None,
        },
        package: input,
        cache: Default::default(),
        may_edit: false,
    };
    let _ = engine::decode_service_reply(&input, bytes);
    let handle: crate::broker::Handle =
        serde_json::from_str("\"ffffffffffffffffffffffffffffffff\"").unwrap();
    let input = crate::broker::Input {
        protocol_version: crate::broker::VERSION,
        request: handle,
        step: 0,
        package: input.package,
        scope: Some(handle),
        may_edit: true,
        workspace_edit: true,
        may_present: true,
        provider: None,
        response: None,
        cache: Default::default(),
    };
    let _ = engine::decode_broker_reply(&input, bytes);
    if let Ok(intent) = serde_json::from_slice::<crate::broker::Intent>(bytes) {
        let _ = crate::broker::Output {
            request: handle,
            step: 0,
            intent,
            cache: Default::default(),
        }
        .validate(&input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_service_admission_preserves_foreground_capacity_without_increasing_the_total() {
        static BUDGET: ServiceBudget = ServiceBudget::new();
        let mut background = Vec::new();
        for _ in 0..crate::service::MAX_ACTIVE - 1 {
            background.push(BUDGET.reserve(true).unwrap());
        }
        assert!(matches!(BUDGET.reserve(true), Err(Error::ServiceBusy)));
        let foreground = BUDGET.reserve(false).unwrap();
        assert!(matches!(BUDGET.reserve(false), Err(Error::ServiceBusy)));
        drop(foreground);
        assert!(BUDGET.available(false));
        assert!(!BUDGET.available(true));
        drop(background.pop());
        drop(BUDGET.reserve(true).unwrap());
        drop(background);
        assert_eq!(BUDGET.active.load(Ordering::Acquire), 0);
        assert_eq!(BUDGET.background.load(Ordering::Acquire), 0);
    }

    #[test]
    fn panicking_host_work_disconnects_before_waking_and_releases_admission() {
        let (sender, receiver) = mpsc::channel();
        let mut worker: Running<(), Error> = spawn(
            move || sender.send(()).unwrap(),
            |_, _| panic!("synthetic host failure"),
        )
        .unwrap();
        receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(worker.try_result(), Some(Err(Error::Stopped)));
    }
    #[test]
    fn async_failure_wakes_the_host_after_the_result_is_queued() {
        let (sender, receiver) = mpsc::channel();
        let mut job = start(
            Input {
                source: String::new(),
                selections: vec![],
                language: String::new(),
                settings: BTreeMap::new(),
            },
            tempfile::tempfile().unwrap(),
            move || sender.send(()).unwrap(),
        )
        .unwrap();
        receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            job.try_result(),
            Some(Err(if SUPPORTED {
                Error::Request
            } else {
                Error::Unavailable
            }))
        );
    }
}
