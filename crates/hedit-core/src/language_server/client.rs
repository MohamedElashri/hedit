//! One stdio JSON-RPC session. All waits happen on a workspace worker.
use super::{
    error,
    manager::{check_cancel, Launch},
    text::{self, Encoding},
    wire::{self, Limits},
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{atomic::AtomicBool, mpsc, Arc},
    time::{Duration, Instant},
};

/// The workspace owner can terminate children immediately when dropped, even
/// if its worker is waiting for a response. Reaping stays on the worker.
#[derive(Default)]
pub(crate) struct Processes {
    pids: std::sync::Mutex<std::collections::HashSet<u32>>,
    closed: AtomicBool,
}
impl Processes {
    fn add(&self, pid: u32) {
        let mut pids = self.pids.lock().unwrap_or_else(|e| e.into_inner());
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            kill_process(pid);
        } else {
            pids.insert(pid);
        }
    }
    fn kill(&self, pid: u32) {
        if !self
            .pids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&pid)
        {
            return;
        }
        kill_process(pid);
    }
    pub fn kill_all(&self) {
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        let pids: Vec<_> = self
            .pids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .collect();
        for pid in pids {
            kill_process(pid);
        }
    }
}
fn kill_process(pid: u32) {
    #[cfg(unix)]
    {
        if let Some(pid) = rustix::process::Pid::from_raw(pid as i32) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE},
        };
        // SAFETY: this PID is registered while the owned Child is alive; the
        // returned handle is used only for termination and closed exactly once.
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !process.is_null() {
                TerminateProcess(process, 1);
                CloseHandle(process);
            }
        }
    }
    let _ = pid;
}
pub(crate) struct Client {
    child: Child,
    input: Option<mpsc::SyncSender<Vec<u8>>>,
    output: mpsc::Receiver<Result<Value, String>>,
    pub encoding: Encoding,
    pub capabilities: Value,
    pub notifications: VecDeque<Value>,
    pub logs: Arc<std::sync::Mutex<VecDeque<String>>>,
    next: u64,
    configuration: Value,
    configuration_section: String,
    root: String,
    _lease: File,
    stopped: bool,
    processes: Arc<Processes>,
}
pub(crate) struct ClientSettings {
    pub configuration: Value,
    pub initialization_options: Value,
    pub environment: std::collections::BTreeMap<String, String>,
    pub section: String,
}
impl Client {
    #[cfg(test)]
    pub fn start(
        launch: Launch,
        lease: File,
        root: &Path,
        configuration: Value,
        cancel: &AtomicBool,
    ) -> Result<Self, String> {
        Self::start_owned(
            launch,
            lease,
            root,
            ClientSettings {
                initialization_options: configuration.clone(),
                configuration,
                environment: Default::default(),
                section: String::new(),
            },
            cancel,
            Arc::new(Processes::default()),
        )
    }
    pub fn start_owned(
        launch: Launch,
        lease: File,
        root: &Path,
        settings: ClientSettings,
        cancel: &AtomicBool,
        processes: Arc<Processes>,
    ) -> Result<Self, String> {
        check_cancel(cancel)?;
        let mut command = Command::new(&launch.executable);
        command
            .args(&launch.arguments)
            .envs(&settings.environment)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start {}: {e}", launch.executable.display()))?;
        processes.add(child.id());
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (input, rx) = mpsc::sync_channel::<Vec<u8>>(8);
        let (tx, output) = mpsc::sync_channel(8);
        let failures = tx.clone();
        std::thread::spawn(move || {
            let mut out = stdin;
            while let Ok(message) = rx.recv() {
                if let Err(e) = write_frame(&mut out, &message) {
                    let _ = failures.send(Err(e));
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader) {
                    Ok(Some(value)) => {
                        if tx.send(Ok(value)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = tx.send(Err("Language server closed its output.".into()));
                        break;
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        let logs = Arc::new(std::sync::Mutex::new(VecDeque::new()));
        let log = logs.clone();
        std::thread::spawn(move || {
            let mut buffer = [0; 2048];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let line = String::from_utf8_lossy(&buffer[..n]).into_owned();
                        if let Ok(mut logs) = log.lock() {
                            logs.push_back(line);
                            while logs.len() > 32 {
                                logs.pop_front();
                            }
                        }
                    }
                }
            }
        });
        let mut client = Self {
            child,
            input: Some(input),
            output,
            encoding: Encoding::Utf16,
            capabilities: Value::Null,
            notifications: VecDeque::new(),
            logs,
            next: 1,
            configuration: settings.configuration,
            configuration_section: settings.section,
            root: text::uri(root)?,
            _lease: lease,
            stopped: false,
            processes,
        };
        let response=client.request("initialize",json!({"processId":std::process::id(),"clientInfo":{"name":"hedit","version":env!("CARGO_PKG_VERSION")},"rootUri":client.root,"workspaceFolders":[{"uri":client.root,"name":root.file_name().unwrap_or_default().to_string_lossy()}],"capabilities":{"general":{"positionEncodings":["utf-8","utf-16","utf-32"]},"workspace":{"configuration":true,"workspaceFolders":true,"applyEdit":false,"workspaceEdit":{"documentChanges":true,"resourceOperations":[]}}, "textDocument":{"inlayHint":{"dynamicRegistration":false},"semanticTokens":{
            "requests":{"full":true},
            "tokenTypes":super::text::SEMANTIC_TYPES,
            "tokenModifiers":["declaration","definition","readonly","static","deprecated","abstract","async","modification","documentation","defaultLibrary"],
            "formats":["relative"],
            "overlappingTokenSupport":false,
            "multilineTokenSupport":false
        },"synchronization":{"didSave":true},"publishDiagnostics":{"versionSupport":true},"completion":{"completionItem":{"snippetSupport":false,"insertReplaceSupport":false}},"hover":{"contentFormat":["plaintext"]},"definition":{"linkSupport":true},"rename":{"prepareSupport":false},"codeAction":{"codeActionLiteralSupport":{"codeActionKind":{"valueSet":["quickfix"]}},"isPreferredSupport":true,"disabledSupport":true}}},"initializationOptions":settings.initialization_options}),cancel,Duration::from_secs(30))?;
        client.encoding = Encoding::parse(response["capabilities"]["positionEncoding"].as_str())?;
        client.capabilities = response["capabilities"].clone();
        client.notify("initialized", json!({}))?;
        client.notify(
            "workspace/didChangeConfiguration",
            json!({"settings":client.configuration}),
        )?;
        Ok(client)
    }
    pub fn supported(&self, capability: &str) -> bool {
        self.capabilities
            .get(capability)
            .is_some_and(|v| !v.is_null() && v != false)
    }
    fn send(&self, value: Value) -> Result<(), String> {
        let sender = self.input.as_ref().ok_or("Language server has stopped.")?;
        let mut message = wire::encode(&value, Limits::BUILTIN).map_err(error)?;
        drop(value);
        let started = Instant::now();
        loop {
            match sender.try_send(message) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err("Language-server input disconnected.".into())
                }
                Err(mpsc::TrySendError::Full(value)) => {
                    if started.elapsed() > Duration::from_secs(2) {
                        return Err("Language-server input remained blocked.".into());
                    }
                    message = value;
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
    pub fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}))
    }
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        let start = Instant::now();
        loop {
            if let Err(e) = check_cancel(cancel) {
                let _ = self.notify("$/cancelRequest", json!({"id":id}));
                return Err(e);
            }
            if start.elapsed() >= timeout {
                let _ = self.notify("$/cancelRequest", json!({"id":id}));
                return Err(format!("Language server timed out answering {method}."));
            }
            match self.output.recv_timeout(Duration::from_millis(20)) {
                Ok(Ok(value)) => {
                    if value["id"] == id && value.get("method").is_none() {
                        if let Some(e) = value.get("error") {
                            return Err(format!(
                                "Language server: {}",
                                e["message"]
                                    .as_str()
                                    .unwrap_or("request failed")
                                    .chars()
                                    .take(4096)
                                    .collect::<String>()
                            ));
                        }
                        return value
                            .get("result")
                            .cloned()
                            .ok_or("Invalid JSON-RPC response.".into());
                    }
                    self.incoming(value)?;
                }
                Ok(Err(e)) => return Err(e),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Language server disconnected.".into())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.child.try_wait().map_err(error)?.is_some() {
                        return Err("Language server exited.".into());
                    }
                }
            }
        }
    }
    fn incoming(&mut self, value: Value) -> Result<(), String> {
        if let Some(method) = value["method"].as_str() {
            if let Some(id) = value.get("id") {
                let response = match method {
                    "workspace/configuration" => {
                        json!({"jsonrpc":"2.0","id":id,"result":value["params"]["items"].as_array().map(|items|items.iter().take(128).map(|i|{
                        let section=i["section"].as_str().unwrap_or("");
                        if section.is_empty(){self.configuration.clone()}else{
                            let value=section.split('.').fold(&self.configuration,|v,k|v.get(k).unwrap_or(&Value::Null));
                            if value.is_null() && section==self.configuration_section{self.configuration.clone()}else{value.clone()}
                        }
                    }).collect::<Vec<_>>()).unwrap_or_default()})
                    }
                    "workspace/workspaceFolders" => {
                        json!({"jsonrpc":"2.0","id":id,"result":[{"uri":self.root,"name":"workspace"}]})
                    }
                    "workspace/applyEdit" => {
                        json!({"jsonrpc":"2.0","id":id,"result":{"applied":false,"failureReason":"Unsolicited workspace edits are not supported."}})
                    }
                    "window/workDoneProgress/create" => {
                        json!({"jsonrpc":"2.0","id":id,"result":null})
                    }
                    _ => {
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Client method is not supported"}})
                    }
                };
                self.send(response)?;
            } else if method == "textDocument/publishDiagnostics" {
                let mut value = value;
                let mut remaining = 256 * 1024;
                if let Some(items) = value["params"]["diagnostics"].as_array_mut() {
                    items.truncate(1000);
                    items.retain(|item| match serde_json::to_vec(item) {
                        Ok(bytes) if bytes.len() <= 8192 && bytes.len() <= remaining => {
                            remaining -= bytes.len();
                            true
                        }
                        _ => false,
                    });
                }
                let uri = value["params"]["uri"].clone();
                self.notifications.retain(|old| old["params"]["uri"] != uri);
                if self.notifications.len() == 32 {
                    self.notifications.pop_front();
                }
                self.notifications.push_back(value);
            } else if matches!(method, "window/logMessage" | "window/showMessage") {
                if let Some(message) = value["params"]["message"].as_str() {
                    if let Ok(mut logs) = self.logs.lock() {
                        logs.push_back(message.chars().take(2048).collect());
                        while logs.len() > 32 {
                            logs.pop_front();
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub fn poll(&mut self) -> Result<(), String> {
        for _ in 0..32 {
            match self.output.try_recv() {
                Ok(Ok(value)) => self.incoming(value)?,
                Ok(Err(e)) => return Err(e),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("Language server disconnected.".into())
                }
            }
        }
        if self.child.try_wait().map_err(error)?.is_some() {
            return Err("Language server exited.".into());
        }
        Ok(())
    }
    pub fn stop(&mut self) {
        // The worker attempts graceful shutdown, then always reaps the child.
        let cancel = AtomicBool::new(false);
        let _ = self.request("shutdown", Value::Null, &cancel, Duration::from_millis(300));
        let _ = self.notify("exit", Value::Null);
        self.kill();
    }
    fn kill(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.input.take();
        self.processes.kill(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.kill();
    }
}

pub(crate) fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut header = Vec::new();
    loop {
        let before = header.len();
        let n = reader
            .take(8193 - before as u64)
            .read_until(b'\n', &mut header)
            .map_err(error)?;
        if n == 0 {
            return if header.is_empty() {
                Ok(None)
            } else {
                Err("Truncated LSP header.".into())
            };
        }
        if header.len() > 8192 || !header[before..].ends_with(b"\r\n") {
            return Err("Invalid or oversized LSP header.".into());
        }
        if &header[before..] == b"\r\n" {
            break;
        }
    }
    let length = wire::content_length(&header, Limits::BUILTIN).map_err(error)?;
    let mut data = vec![0; length];
    reader.read_exact(&mut data).map_err(error)?;
    wire::parse_json(&data, Limits::BUILTIN)
        .map(Some)
        .map_err(error)
}
fn write_frame(out: &mut impl Write, frame: &[u8]) -> Result<(), String> {
    out.write_all(frame).map_err(error)?;
    out.flush().map_err(error)
}
