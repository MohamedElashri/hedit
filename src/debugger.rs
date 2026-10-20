//! Native CUDA-GDB/MI transport and bounded tool output. Commands never use a shell.
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub const OUTPUT_LIMIT: usize = 512 * 1024;
const RECORD_LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ready,
    Running,
    Stopped,
    Exited,
}

pub struct DebugSession {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<String>,
    dropped: Arc<AtomicUsize>,
    pub transcript: String,
    pub state: State,
    pub mi: bool,
    pub exit_code: Option<i32>,
    token: u64,
    stopping: Option<Instant>,
}

pub fn quote(value: &str) -> String {
    // GDB MI accepts C strings; JSON's common escapes are the same. Keep
    // non-ASCII text literal rather than generating unsupported \u escapes.
    serde_json::to_string(value).expect("a string is serializable")
}

impl DebugSession {
    pub fn launch(
        debugger: &str,
        executable: &Path,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<Self, String> {
        if !executable.is_file() {
            return Err("Choose an existing CUDA executable (compile with nvcc -g -G).".into());
        }
        let mut command = Command::new(debugger);
        command
            .args(["--nx", "--quiet", "--interpreter=mi2", "--args"])
            .arg(executable)
            .args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut session = Self::spawn(command, true)?;
        session.send("-gdb-set mi-async on")?;
        session.send("-gdb-set pagination off")?;
        session.send("-break-insert -f main")?;
        Ok(session)
    }
    pub fn sanitizer(executable: &Path, args: &[String], tool: &str) -> Result<Self, String> {
        if !matches!(tool, "memcheck" | "racecheck" | "initcheck" | "synccheck") {
            return Err("Unknown Compute Sanitizer tool.".into());
        }
        if !executable.is_file() {
            return Err("Choose an existing executable.".into());
        }
        let mut command = Command::new("compute-sanitizer");
        command
            .args(["--tool", tool, "--error-exitcode", "1"])
            .arg(executable)
            .args(args);
        Self::spawn(command, false)
    }
    pub fn gpu_info() -> Result<Self, String> {
        let mut command = Command::new("nvidia-smi");
        command.args([
            "--query-gpu=name,driver_version,memory.used,memory.total,temperature.gpu",
            "--format=csv",
        ]);
        Self::spawn(command, false)
    }
    fn spawn(mut command: Command, mi: bool) -> Result<Self, String> {
        let name = command.get_program().to_string_lossy().into_owned();
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!("Could not start {name}: {e}. Install the NVIDIA tool and add it to PATH.")
            })?;
        let input = child.stdin.take();
        let (sender, output) = mpsc::sync_channel(256);
        let dropped = Arc::new(AtomicUsize::new(0));
        if let Some(stdout) = child.stdout.take() {
            read_output(stdout, sender.clone(), Arc::clone(&dropped));
        }
        if let Some(stderr) = child.stderr.take() {
            read_output(stderr, sender, Arc::clone(&dropped));
        }
        Ok(Self {
            child,
            input,
            output,
            dropped,
            transcript: String::new(),
            state: if mi { State::Ready } else { State::Running },
            mi,
            exit_code: None,
            token: 0,
            stopping: None,
        })
    }
    pub fn send(&mut self, command: &str) -> Result<(), String> {
        if !self.mi {
            return Err("This tool has no debugger command input.".into());
        }
        if self.state == State::Exited {
            return Err("Debugger has exited.".into());
        }
        if command.contains(['\n', '\r', '\0']) || command.len() > 8192 {
            return Err("Enter a single debugger command of at most 8192 bytes.".into());
        }
        self.token += 1;
        let input = self.input.as_mut().ok_or("Debugger input is closed.")?;
        writeln!(input, "{}{command}", self.token)
            .and_then(|_| input.flush())
            .map_err(|e| e.to_string())
    }
    pub fn console(&mut self, command: &str) -> Result<(), String> {
        if command.contains(['\n', '\r', '\0']) {
            return Err("Enter one console command.".into());
        }
        self.send(&format!("-interpreter-exec console {}", quote(command)))
    }
    pub fn breakpoint(&mut self, location: &str) -> Result<(), String> {
        if location.trim().is_empty() {
            return Err("Enter a function or file:line breakpoint.".into());
        }
        self.send(&format!("-break-insert -f {}", quote(location)))
    }
    pub fn stop(&mut self) {
        if self.state == State::Exited {
            return;
        }
        if self.mi {
            let _ = self.send("-gdb-exit");
        } else {
            let _ = self.child.kill();
        }
        self.stopping = Some(Instant::now());
    }
    pub fn poll(&mut self) {
        let dropped = self.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            self.append(&format!(
                "\n[Output truncated: {dropped} records dropped]\n"
            ));
        }
        for _ in 0..512 {
            let Ok(record) = self.output.try_recv() else {
                break;
            };
            if self.mi {
                if record.starts_with("*running") {
                    self.state = State::Running;
                }
                if record.starts_with("*stopped") {
                    self.state = State::Stopped;
                }
                let bytes = record.as_bytes();
                let display = if matches!(bytes.first(), Some(b'~' | b'@' | b'&')) {
                    decode_stream(record[1..].trim_end()).unwrap_or(record.clone())
                } else {
                    record
                };
                self.append(&display);
            } else {
                self.append(&record);
            }
        }
        if self
            .stopping
            .is_some_and(|at| at.elapsed() > Duration::from_secs(1))
        {
            let _ = self.child.kill();
        }
        if let Ok(Some(status)) = self.child.try_wait() {
            if self.state != State::Exited {
                self.exit_code = status.code();
                self.state = State::Exited;
                self.append(&format!("\n[Process exited: {status}]\n"));
            }
        }
    }
    fn append(&mut self, text: &str) {
        self.transcript.push_str(text);
        if self.transcript.len() > OUTPUT_LIMIT {
            let mut start = self.transcript.len() - OUTPUT_LIMIT;
            while !self.transcript.is_char_boundary(start) {
                start += 1;
            }
            self.transcript.drain(..start);
        }
    }
}
impl Drop for DebugSession {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            if self.mi {
                let _ = self.send("-gdb-exit");
            }
            // Child ownership cannot move out of Drop. Give the orderly exit a
            // short grace period, then kill and reap the debugger process.
            for _ in 0..10 {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
fn read_output(
    mut input: impl Read + Send + 'static,
    output: SyncSender<String>,
    dropped: Arc<AtomicUsize>,
) {
    thread::spawn(move || {
        let mut chunk = [0; 4096];
        let mut record = Vec::new();
        loop {
            match input.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    for byte in &chunk[..count] {
                        record.push(*byte);
                        if *byte == b'\n' || record.len() >= RECORD_LIMIT {
                            if output
                                .try_send(String::from_utf8_lossy(&record).into_owned())
                                .is_err()
                            {
                                dropped.fetch_add(1, Ordering::Relaxed);
                            }
                            record.clear();
                        }
                    }
                }
            }
        }
        if !record.is_empty() {
            let _ = output.try_send(String::from_utf8_lossy(&record).into_owned());
        }
    });
}

/// Decode MI's C escapes, including octal bytes used for UTF-8 console text.
pub fn decode_stream(value: &str) -> Option<String> {
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut result = Vec::new();
    let mut bytes = value.bytes().peekable();
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            result.push(byte);
            continue;
        }
        let escaped = bytes.next()?;
        match escaped {
            b'n' => result.push(b'\n'),
            b'r' => result.push(b'\r'),
            b't' => result.push(b'\t'),
            b'"' | b'\\' => result.push(escaped),
            b'0'..=b'7' => {
                let mut number = (escaped - b'0') as u16;
                for _ in 0..2 {
                    if let Some(next @ b'0'..=b'7') = bytes.peek().copied() {
                        bytes.next();
                        number = number * 8 + (next - b'0') as u16;
                    } else {
                        break;
                    }
                }
                result.push(u8::try_from(number).ok()?);
            }
            other => {
                result.push(b'\\');
                result.push(other);
            }
        }
    }
    Some(String::from_utf8_lossy(&result).into_owned())
}
