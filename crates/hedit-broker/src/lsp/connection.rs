use hedit_core::language_server::wire::{self, Decoder, Limits, READ_BYTES};
use hedit_plugin::tool::Progress;
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, VecDeque},
    io,
    sync::atomic::AtomicBool,
    time::Instant,
};

const QUEUE_BYTES: usize = 1024 * 1024;
const MESSAGES: usize = 64;
const REQUESTS: u32 = 32_768;
pub trait Transport {
    fn poll(
        &mut self,
        input: &[u8],
        output: &mut [u8; READ_BYTES],
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Progress>;
    fn wait(&self, input_pending: bool) -> io::Result<()>;
}
impl Transport for crate::tool::Service {
    fn poll(
        &mut self,
        input: &[u8],
        output: &mut [u8; READ_BYTES],
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Progress> {
        self.poll(input, output, cancel, deadline)
    }
    fn wait(&self, input_pending: bool) -> io::Result<()> {
        self.wait(input_pending)
    }
}
#[derive(Clone, Copy)]
pub enum Method {
    Diagnostics,
    Completions,
    Hover,
    Symbols,
    Formatting,
    CodeActions,
}
impl Method {
    fn name(self) -> &'static str {
        match self {
            Self::Diagnostics => "textDocument/diagnostic",
            Self::Completions => "textDocument/completion",
            Self::Hover => "textDocument/hover",
            Self::Symbols => "textDocument/documentSymbol",
            Self::Formatting => "textDocument/formatting",
            Self::CodeActions => "textDocument/codeAction",
        }
    }
}
#[derive(Clone, Copy)]
pub enum Notification {
    Open,
    Change,
    Close,
}
impl Notification {
    fn name(self) -> &'static str {
        match self {
            Self::Open => "textDocument/didOpen",
            Self::Change => "textDocument/didChange",
            Self::Close => "textDocument/didClose",
        }
    }
}
struct Frame {
    bytes: Vec<u8>,
    sent: usize,
    request: Option<u32>,
}
/// One serial JSON-RPC connection. Requests never share an ID, and an error
/// destroys the native owner. Superseded IDs remain fenced until their late
/// reply is discarded; scope cancellation still destroys the owner. Server requests receive MethodNotFound, never host effects.
pub struct Connection<T: Transport> {
    transport: Option<T>,
    decoder: Decoder,
    queue: VecDeque<Frame>,
    queued: usize,
    diagnostics: VecDeque<Value>,
    retained: usize,
    next: u32,
    initialized: bool,
    diagnostic_scope: Option<(String, i32)>,
    idle_messages: usize,
    idle_bytes: usize,
    retired: BTreeSet<u32>,
    soft_cancelled: bool,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid scoped language-server exchange.",
    )
}
impl<T: Transport> Connection<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport: Some(transport),
            decoder: Decoder::new(Limits::SCOPED),
            queue: VecDeque::new(),
            queued: 0,
            diagnostics: VecDeque::new(),
            retained: 0,
            next: 0,
            initialized: false,
            diagnostic_scope: None,
            idle_messages: 0,
            idle_bytes: 0,
            retired: BTreeSet::new(),
            soft_cancelled: false,
        }
    }
    pub(super) fn open(&self) -> bool {
        self.transport.is_some()
    }
    fn live(&self) -> io::Result<()> {
        self.transport.as_ref().map(|_| ()).ok_or_else(invalid)
    }
    pub(super) fn transport_mut(&mut self) -> io::Result<&mut T> {
        self.transport.as_mut().ok_or_else(invalid)
    }
    pub(super) fn diagnostic_scope(&mut self, uri: &str, version: i32) {
        self.diagnostic_scope = Some((uri.into(), version));
        self.diagnostics.clear();
        self.retained = 0;
    }
    fn poison<R>(&mut self, result: io::Result<R>) -> io::Result<R> {
        if result.is_err() {
            self.transport.take();
            self.queue.clear();
            self.queued = 0;
            self.diagnostics.clear();
            self.retained = 0;
            self.retired.clear();
        }
        result
    }
    fn enqueue(&mut self, value: &Value) -> io::Result<()> {
        self.live()?;
        let bytes = wire::encode(value, Limits::SCOPED)?;
        if self.queue.len() >= 8 || bytes.len() > QUEUE_BYTES.saturating_sub(self.queued) {
            return Err(invalid());
        }
        self.queued += bytes.len();
        let request = value
            .get("method")
            .and_then(|_| value.get("id"))
            .and_then(Value::as_u64)
            .and_then(|id| u32::try_from(id).ok());
        self.queue.push_back(Frame {
            bytes,
            sent: 0,
            request,
        });
        Ok(())
    }
    pub fn initialize(
        &mut self,
        params: Value,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Value> {
        let result = (|| {
            if self.initialized || self.next != 0 {
                return Err(invalid());
            }
            let result = self.exchange("initialize", params, cancel, deadline, None)?;
            if !result.get("capabilities").is_some_and(Value::is_object) {
                return Err(invalid());
            }
            self.initialized = true;
            self.enqueue(&json!({"jsonrpc":"2.0", "method":"initialized", "params":{}}))?;
            self.pump(None, cancel, deadline, None)?;
            Ok(result)
        })();
        self.poison(result)
    }
    pub fn notify(
        &mut self,
        method: Notification,
        params: Value,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<()> {
        let result = (|| {
            self.idle_messages = 0;
            self.idle_bytes = 0;
            if !self.initialized {
                return Err(invalid());
            }
            self.enqueue(&json!({"jsonrpc":"2.0", "method":method.name(), "params":params}))?;
            self.pump(None, cancel, deadline, None)?;
            Ok(())
        })();
        self.poison(result)
    }
    pub fn request(
        &mut self,
        method: Method,
        params: Value,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Value> {
        self.request_with(method, params, cancel, None, deadline)
    }
    /// A superseded callback fences its already-delivered request ID and sends
    /// $/cancelRequest. Scope cancellation and protocol failures still kill.
    pub fn request_cancellable(
        &mut self,
        method: Method,
        params: Value,
        cancel: &AtomicBool,
        superseded: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Value> {
        self.request_with(method, params, cancel, Some(superseded), deadline)
    }
    fn request_with(
        &mut self,
        method: Method,
        params: Value,
        cancel: &AtomicBool,
        superseded: Option<&AtomicBool>,
        deadline: Instant,
    ) -> io::Result<Value> {
        self.soft_cancelled = false;
        let result = if self.initialized {
            self.exchange(method.name(), params, cancel, deadline, superseded)
        } else {
            Err(invalid())
        };
        if self.soft_cancelled && !cancel.load(std::sync::atomic::Ordering::Acquire) && self.open()
        {
            result
        } else {
            self.poison(result)
        }
    }
    fn exchange(
        &mut self,
        method: &str,
        params: Value,
        cancel: &AtomicBool,
        deadline: Instant,
        superseded: Option<&AtomicBool>,
    ) -> io::Result<Value> {
        self.live()?;
        self.idle_messages = 0;
        self.idle_bytes = 0;
        // Reject queued late responses before assigning the next request ID.
        self.receive(None, &mut None, &mut 0)?;
        self.next = self
            .next
            .checked_add(1)
            .filter(|id| *id <= REQUESTS)
            .ok_or_else(invalid)?;
        self.enqueue(&json!({"jsonrpc":"2.0", "id":self.next, "method":method, "params":params}))?;
        self.pump(Some(self.next), cancel, deadline, superseded)?
            .ok_or_else(invalid)
    }
    /// Drive idle input on the service's host I/O thread. Callers retain no
    /// authority guard while waiting; raw notifications cannot publish results.
    pub fn idle(&mut self, cancel: &AtomicBool, deadline: Instant) -> io::Result<()> {
        let result = self.pump(None, cancel, deadline, None).map(|_| ());
        self.poison(result)
    }
    pub fn take_diagnostics(&mut self) -> VecDeque<Value> {
        self.retained = 0;
        std::mem::take(&mut self.diagnostics)
    }
    fn receive(
        &mut self,
        pending: Option<u32>,
        response: &mut Option<Value>,
        messages: &mut usize,
    ) -> io::Result<()> {
        while let Some(mut value) = self.decoder.next_message()? {
            *messages += 1;
            if *messages > MESSAGES {
                return Err(invalid());
            }
            if let Some(method) = value.get("method").and_then(Value::as_str) {
                if let Some(id) = value.get("id") {
                    self.enqueue(&json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32601,"message":"Server requests are unavailable."}}))?;
                } else if method == "textDocument/publishDiagnostics" {
                    if self
                        .diagnostic_scope
                        .as_ref()
                        .is_some_and(|(uri, version)| {
                            value["params"]["uri"].as_str() != Some(uri)
                                || value["params"]["version"].as_i64() != Some(i64::from(*version))
                        })
                    {
                        continue;
                    }
                    let bytes = serde_json::to_vec(&value).map_err(|_| invalid())?.len();
                    if self.diagnostics.len() >= 16
                        || bytes > (256 * 1024usize).saturating_sub(self.retained)
                    {
                        return Err(invalid());
                    }
                    self.retained += bytes;
                    self.diagnostics.push_back(value);
                }
            } else {
                if let Some(id) = value
                    .get("id")
                    .and_then(Value::as_u64)
                    .and_then(|id| u32::try_from(id).ok())
                {
                    if self.retired.remove(&id) {
                        continue;
                    }
                }
                if pending.is_none()
                    || response.is_some()
                    || value.get("id").and_then(Value::as_u64) != pending.map(u64::from)
                    || value.get("error").is_some()
                {
                    return Err(invalid());
                }
                *response = Some(value.get_mut("result").ok_or_else(invalid)?.take());
            }
        }
        Ok(())
    }
    fn pump(
        &mut self,
        pending: Option<u32>,
        cancel: &AtomicBool,
        deadline: Instant,
        superseded: Option<&AtomicBool>,
    ) -> io::Result<Option<Value>> {
        let mut response = None;
        let mut request_sent = pending.is_none();
        let mut messages = if pending.is_none() {
            self.idle_messages
        } else {
            0
        };
        let mut bytes = if pending.is_none() {
            self.idle_bytes
        } else {
            0
        };
        loop {
            self.live()?;
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Scoped language-server request cancelled.",
                ));
            }
            let superseded_now =
                superseded.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire));
            if superseded_now {
                if let Some(id) = pending {
                    if request_sent {
                        if response.is_none() {
                            if self.retired.len() >= 16 {
                                return Err(invalid());
                            }
                            self.retired.insert(id);
                            self.enqueue(&json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":id}}))?;
                        }
                        self.soft_cancelled = true;
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "Scoped language-server callback superseded.",
                        ));
                    }
                    // An unsent request can be removed. A partially written
                    // frame must finish under the caller's disclosure guard.
                    if let Some(index) = self
                        .queue
                        .iter()
                        .position(|frame| frame.request == Some(id) && frame.sent == 0)
                    {
                        let frame = self.queue.remove(index).ok_or_else(invalid)?;
                        self.queued -= frame.bytes.len();
                        self.soft_cancelled = true;
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "Scoped language-server callback superseded.",
                        ));
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Scoped language-server request timed out.",
                ));
            }
            let input = self
                .queue
                .front()
                .map_or(&[][..], |frame| &frame.bytes[frame.sent..]);
            let mut output = [0; READ_BYTES];
            let progress = self.transport.as_mut().ok_or_else(invalid)?.poll(
                input,
                &mut output,
                cancel,
                deadline,
            )?;
            if progress.written > input.len() || progress.read > READ_BYTES {
                return Err(invalid());
            }
            if let Some(frame) = self.queue.front_mut() {
                frame.sent += progress.written;
                if frame.sent == frame.bytes.len() {
                    if pending.is_some() && frame.request == pending {
                        request_sent = true;
                    }
                    self.queued -= frame.bytes.len();
                    self.queue.pop_front();
                }
            }
            bytes += progress.read + progress.written;
            if bytes > 2 * QUEUE_BYTES {
                return Err(invalid());
            }
            self.decoder.push(&output[..progress.read])?;
            // A response is valid only after the complete client request was
            // delivered. Pending server-request denials cannot create authority.
            self.receive(
                if request_sent { pending } else { None },
                &mut response,
                &mut messages,
            )?;
            if pending.is_none() {
                self.idle_messages = messages;
                self.idle_bytes = bytes;
            }
            if self.queue.is_empty() && (pending.is_none() || response.is_some()) {
                if pending.is_some()
                    && superseded
                        .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
                {
                    self.soft_cancelled = true;
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Scoped language-server callback superseded.",
                    ));
                }
                return Ok(response);
            }
            if progress.idle() {
                self.transport
                    .as_ref()
                    .ok_or_else(invalid)?
                    .wait(!self.queue.is_empty())?;
            }
        }
    }
}

#[cfg(test)]
mod tests;
