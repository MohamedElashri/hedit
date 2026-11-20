use super::{Input, HELPER_ARG, MAX_SELECTIONS, MAX_SELECTION_BYTES};
use hedit::plugin_options::{self, Value as SettingValue};
use mlua::chunk::ChunkMode;
use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};
use std::cell::Cell;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::thread;
use std::time::{Duration, Instant};

const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_LANGUAGE_BYTES: usize = 256;
const MAX_REPLACEMENT_BYTES: usize = 256 * 1024;
const MAX_BATCH_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_BATCH_REPLY_BYTES: usize = 2 * 1024 * 1024;
const BATCH_REQUEST: u32 = u32::MAX - 1;
const LUA_MEMORY_BYTES: usize = 16 * 1024 * 1024;
const INSTRUCTION_INTERVAL: u32 = 1_000;
const INSTRUCTION_BUDGET: u32 = 1_000_000;
const DEADLINE: Duration = Duration::from_secs(2);
const CHECK_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
enum Failure {
    Sandbox = 1,
    Request,
    Initialization,
    Syntax,
    EntryPoint,
    Execution,
    Instructions,
    Memory,
    ReturnType,
    ReturnEncoding,
    ReturnSize,
    Worker,
    Deadline,
    Cancelled,
}

impl Failure {
    fn message(self) -> &'static str {
        match self {
            Self::Sandbox => "The OS could not isolate the Lua worker.",
            Self::Request => "The Lua plugin input is invalid or exceeds its limits.",
            Self::Initialization => "The Lua runtime could not start.",
            Self::Syntax => "The plugin has a Lua syntax error.",
            Self::EntryPoint => "The Lua script must return a function.",
            Self::Execution => "The Lua plugin raised an error.",
            Self::Instructions => "The Lua plugin reached its instruction limit.",
            Self::Memory => "The Lua plugin reached its memory limit.",
            Self::ReturnType => "The Lua plugin must return a string.",
            Self::ReturnEncoding => "The plugin replacement must be UTF-8 text without NUL bytes.",
            Self::ReturnSize => "The Lua plugin replacement exceeds 256 KiB.",
            Self::Worker => "The Lua worker stopped or returned an invalid response.",
            Self::Deadline => "The Lua plugin exceeded its time limit.",
            Self::Cancelled => "The Lua plugin was cancelled.",
        }
    }

    fn from_worker_code(code: u8) -> Self {
        match code {
            1 => Self::Sandbox,
            2 => Self::Request,
            3 => Self::Initialization,
            4 => Self::Syntax,
            5 => Self::EntryPoint,
            6 => Self::Execution,
            7 => Self::Instructions,
            8 => Self::Memory,
            9 => Self::ReturnType,
            10 => Self::ReturnEncoding,
            11 => Self::ReturnSize,
            _ => Self::Worker,
        }
    }
}

impl Input {
    fn validate(&self) -> Result<(), Failure> {
        if self.source.len() > MAX_SOURCE_BYTES
            || self.language.len() > MAX_LANGUAGE_BYTES
            || !(1..=MAX_SELECTIONS).contains(&self.selections.len())
            || self.selections.iter().map(String::len).sum::<usize>() > MAX_SELECTION_BYTES
            || self.settings.len() > plugin_options::MAX_SETTINGS
            || self
                .settings
                .iter()
                .any(|(key, value)| !plugin_options::valid_id(key) || !value.valid())
        {
            return Err(Failure::Request);
        }
        Ok(())
    }
}

fn encode_request(input: &Input) -> Result<Vec<u8>, Failure> {
    input.validate()?;
    let payload = serde_json::to_vec(input).map_err(|_| Failure::Request)?;
    if payload.len() > MAX_BATCH_REQUEST_BYTES {
        return Err(Failure::Request);
    }
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend(BATCH_REQUEST.to_le_bytes());
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
    Ok(bytes)
}

fn decode_request(mut input: impl Read) -> Result<(Input, bool), Failure> {
    fn length(input: &mut impl Read) -> Result<u32, Failure> {
        let mut size = [0; 4];
        input.read_exact(&mut size).map_err(|_| Failure::Request)?;
        Ok(u32::from_le_bytes(size))
    }

    fn bytes(input: &mut impl Read, size: u32, maximum: usize) -> Result<Vec<u8>, Failure> {
        if size as usize > maximum {
            return Err(Failure::Request);
        }
        let mut bytes = vec![0; size as usize];
        input.read_exact(&mut bytes).map_err(|_| Failure::Request)?;
        Ok(bytes)
    }

    fn text(input: &mut impl Read, size: u32, maximum: usize) -> Result<String, Failure> {
        String::from_utf8(bytes(input, size, maximum)?).map_err(|_| Failure::Request)
    }

    let first = length(&mut input)?;
    let batch = first == BATCH_REQUEST;
    let request = if batch {
        let size = length(&mut input)?;
        let payload = bytes(&mut input, size, MAX_BATCH_REQUEST_BYTES)?;
        serde_json::from_slice(&payload).map_err(|_| Failure::Request)?
    } else {
        let source = text(&mut input, first, MAX_SOURCE_BYTES)?;
        let size = length(&mut input)?;
        let selection = text(&mut input, size, MAX_SELECTION_BYTES)?;
        let size = length(&mut input)?;
        let language = text(&mut input, size, MAX_LANGUAGE_BYTES)?;
        Input {
            source,
            selections: vec![selection],
            language,
            settings: Default::default(),
        }
    };
    if input.read(&mut [0; 1]).map_err(|_| Failure::Request)? != 0 {
        return Err(Failure::Request);
    }
    request.validate()?;
    Ok((request, batch))
}

pub(super) fn run(
    input: Input,
    approval: Option<File>,
    cancelled: &AtomicBool,
    save: bool,
) -> Result<Vec<String>, String> {
    let count = input.selections.len();
    let response = (|| {
        let request = encode_request(&input)?;
        let timeout = if save {
            Duration::from_millis(400)
        } else {
            DEADLINE
        };
        let reply = supervise(
            Path::new("/proc/self/exe"),
            request,
            cancelled,
            timeout,
            approval,
        )?;
        let edits: Vec<String> = serde_json::from_slice(&reply).map_err(|_| Failure::Worker)?;
        if edits.len() != count {
            return Err(Failure::Worker);
        }
        validate_edits(&edits)?;
        Ok(edits)
    })();
    response.map_err(|failure| failure.message().to_owned())
}

struct ReapChild(Child);

impl Drop for ReapChild {
    fn drop(&mut self) {
        // Also reap the child after a cancelled run or an IPC error.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn set_nonblocking(pipe: impl AsFd) -> Result<(), Failure> {
    let fd = pipe.as_fd().as_raw_fd();
    // The pipe remains open while we change its status flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(Failure::Worker);
    }
    Ok(())
}

fn supervise(
    executable: &Path,
    request: Vec<u8>,
    cancelled: &AtomicBool,
    timeout: Duration,
    mut approval: Option<File>,
) -> Result<Vec<u8>, Failure> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(Failure::Cancelled);
    }
    let deadline = Instant::now() + timeout;
    let mut child = ReapChild(
        Command::new(executable)
            .arg(HELPER_ARG)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Failure::Worker)?,
    );
    let input = child.0.stdin.take().ok_or(Failure::Worker)?;
    let output = child.0.stdout.take().ok_or(Failure::Worker)?;
    set_nonblocking(&input)?;
    set_nonblocking(&output)?;
    let mut input = Some(input);
    let output_fd = output.as_raw_fd();
    let mut output = output.take(MAX_BATCH_REPLY_BYTES as u64 + 2);
    let mut sent = 0;
    let mut reply = Vec::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Failure::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Failure::Deadline);
        }
        if let Some(stdin) = input.as_mut() {
            match stdin.write(&request[sent..]) {
                Ok(written) => {
                    sent += written;
                    if sent == request.len() {
                        // EOF is part of the request framing.
                        input = None;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => input = None,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Err(Failure::Worker),
            }
        }

        // Serialize revocation with delivery of the selection to the worker.
        // Once delivery ends, a revoke can proceed while computation runs.
        if input.is_none() {
            drop(approval.take());
        }
        let status = child.0.try_wait().map_err(|_| Failure::Worker)?;
        let output_open = match output.read_to_end(&mut reply) {
            Ok(_) => false,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => true,
            Err(_) => return Err(Failure::Worker),
        };
        if reply.len() > MAX_BATCH_REPLY_BYTES + 1 {
            return Err(Failure::Worker);
        }
        if let Some(status) = status {
            if !status.success() {
                return Err(Failure::Worker);
            }
            break;
        }

        let mut pipes = [
            libc::pollfd {
                fd: input.as_ref().map_or(-1, AsRawFd::as_raw_fd),
                events: libc::POLLOUT,
                revents: 0,
            },
            libc::pollfd {
                fd: if output_open { output_fd } else { -1 },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // The pipes are owned here, and poll ignores completed (-1) descriptors.
        // Bound the wait even when a child stops reading or writing mid-message.
        let _ = unsafe {
            libc::poll(
                pipes.as_mut_ptr(),
                pipes.len() as libc::nfds_t,
                CHECK_INTERVAL.as_millis() as i32,
            )
        };
    }

    let Some((&status, body)) = reply.split_first() else {
        return Err(Failure::Worker);
    };
    if status != 0 {
        return Err(if body.is_empty() {
            Failure::from_worker_code(status)
        } else {
            Failure::Worker
        });
    }
    if sent != request.len() {
        return Err(Failure::Worker);
    }
    Ok(reply.split_off(1))
}

pub(super) fn run_helper(isolated: bool) -> bool {
    // No attacker-influenced request bytes or Lua code are consumed before
    // the process is isolated. Failure is reported through a fixed status code.
    let result = if isolated {
        decode_request(io::stdin().lock()).and_then(|(input, batch)| {
            let edits = evaluate(input, batch)?;
            if batch {
                serde_json::to_vec(&edits).map_err(|_| Failure::Worker)
            } else {
                Ok(edits
                    .into_iter()
                    .next()
                    .ok_or(Failure::Worker)?
                    .into_bytes())
            }
        })
    } else {
        Err(Failure::Sandbox)
    };
    let (status, body) = match &result {
        Ok(body) => (0, body.as_slice()),
        Err(failure) => (*failure as u8, &[][..]),
    };
    let mut output = io::stdout().lock();
    output
        .write_all(&[status])
        .and_then(|_| output.write_all(body))
        .and_then(|_| output.flush())
        .is_ok()
}

fn evaluate(request: Input, batch: bool) -> Result<Vec<String>, Failure> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|_| Failure::Initialization)?;
    lua.set_memory_limit(LUA_MEMORY_BYTES)
        .map_err(|_| Failure::Initialization)?;
    let environment = environment(&lua).map_err(|_| Failure::Initialization)?;

    let remaining = Rc::new(Cell::new(INSTRUCTION_BUDGET));
    let hook_remaining = remaining.clone();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(INSTRUCTION_INTERVAL),
        move |_, _| {
            let left = hook_remaining.get().saturating_sub(INSTRUCTION_INTERVAL);
            hook_remaining.set(left);
            if left == 0 {
                return Err(mlua::Error::RuntimeError("instruction limit".into()));
            }
            Ok(VmState::Continue)
        },
    )
    .map_err(|_| Failure::Initialization)?;

    let result = call_transform(&lua, environment, request, batch);
    if remaining.get() == 0 {
        return Err(Failure::Instructions);
    }
    result
}

fn call_transform(
    lua: &Lua,
    environment: Table,
    request: Input,
    batch: bool,
) -> Result<Vec<String>, Failure> {
    let entry = lua
        .load(request.source.as_bytes())
        .set_name("plugin")
        .set_mode(ChunkMode::Text)
        .set_environment(environment)
        .into_function()
        .map_err(lua_error)?
        .call::<Value>(())
        .map_err(lua_error)?;
    let Value::Function(entry) = entry else {
        return Err(Failure::EntryPoint);
    };
    let mut edits = Vec::with_capacity(request.selections.len());
    for selection in &request.selections {
        let result = if batch {
            let settings = lua.create_table().map_err(lua_error)?;
            for (key, value) in &request.settings {
                match value {
                    SettingValue::Boolean(value) => settings.raw_set(key.as_str(), *value),
                    SettingValue::Integer(value) => settings.raw_set(key.as_str(), *value),
                    SettingValue::Text(value) => settings.raw_set(key.as_str(), value.as_str()),
                }
                .map_err(lua_error)?;
            }
            entry.call::<Value>((selection.as_str(), request.language.as_str(), settings))
        } else {
            entry.call::<Value>((selection.as_str(), request.language.as_str()))
        }
        .map_err(lua_error)?;
        let Value::String(replacement) = result else {
            return Err(Failure::ReturnType);
        };
        edits.push(valid_replacement(&replacement.as_bytes())?);
        validate_edits(&edits)?;
    }
    Ok(edits)
}

fn validate_edits(edits: &[String]) -> Result<(), Failure> {
    if edits.iter().map(String::len).sum::<usize>() > MAX_REPLACEMENT_BYTES {
        return Err(Failure::ReturnSize);
    }
    if edits.iter().any(|edit| edit.contains('\0')) {
        return Err(Failure::ReturnEncoding);
    }
    Ok(())
}

fn environment(lua: &Lua) -> mlua::Result<Table> {
    let globals = lua.globals();
    let environment = lua.create_table()?;
    // The string metatable points to this same library table.
    globals
        .raw_get::<Table>("string")?
        .raw_set("dump", Value::Nil)?;
    for name in [
        "_VERSION", "assert", "error", "ipairs", "next", "pairs", "select", "tonumber", "tostring",
        "type", "math", "string", "table", "utf8",
    ] {
        environment.raw_set(name, globals.raw_get::<Value>(name)?)?;
    }
    Ok(environment)
}

fn lua_error(error: mlua::Error) -> Failure {
    match error {
        mlua::Error::SyntaxError { .. } => Failure::Syntax,
        mlua::Error::MemoryError(_) => Failure::Memory,
        _ => Failure::Execution,
    }
}

fn valid_replacement(bytes: &[u8]) -> Result<String, Failure> {
    if bytes.len() > MAX_REPLACEMENT_BYTES {
        return Err(Failure::ReturnSize);
    }
    if bytes.contains(&0) {
        return Err(Failure::ReturnEncoding);
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| Failure::ReturnEncoding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn fixture(dir: &Path, script: &str) -> std::path::PathBuf {
        let worker = dir.join("test-worker");
        fs::write(&worker, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
        worker
    }

    #[test]
    fn supervisor_exchanges_and_validates_bounded_pipes() {
        let dir = tempfile::tempdir().unwrap();
        let worker = fixture(dir.path(), "exec /bin/cat");
        let text = "café".repeat(18_000);
        let reply = [b"\0".as_slice(), text.as_bytes()].concat();
        assert_eq!(
            supervise(&worker, reply, &AtomicBool::new(false), DEADLINE, None),
            Ok(text.into_bytes()),
        );
    }

    #[test]
    fn supervisor_holds_approval_only_through_selection_delivery() {
        let dir = tempfile::tempdir().unwrap();
        let worker = fixture(
            dir.path(),
            r#"
: > "$0.started"
while [ ! -e "$0.read" ]; do /bin/sleep 0.01; done
/bin/cat >/dev/null
: > "$0.received"
while [ ! -e "$0.finish" ]; do /bin/sleep 0.01; done
printf '\000'
"#,
        );
        let approval = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let competing = approval.reopen().unwrap();
        approval.as_file().try_lock().unwrap();
        let cancelled = AtomicBool::new(false);
        let marker = |name| worker.with_extension(name);
        fn wait(mut condition: impl FnMut() -> bool) {
            let until = Instant::now() + DEADLINE;
            while !condition() {
                assert!(
                    Instant::now() < until,
                    "helper did not reach expected phase"
                );
                thread::sleep(CHECK_INTERVAL);
            }
        }
        thread::scope(|scope| {
            let run = scope.spawn(|| {
                // Large enough that the entire request cannot fit in the pipe.
                supervise(
                    &worker,
                    vec![b'x'; 2 * 1024 * 1024],
                    &cancelled,
                    DEADLINE,
                    Some(approval.into_file()),
                )
            });
            wait(|| marker("started").is_file());
            assert!(competing.try_lock().is_err());
            fs::write(marker("read"), []).unwrap();
            wait(|| marker("received").is_file() && competing.try_lock().is_ok());
            assert!(!run.is_finished());
            fs::write(marker("finish"), []).unwrap();
            assert_eq!(run.join().unwrap(), Ok(Vec::new()));
        });
    }

    #[test]
    fn supervisor_enforces_wall_clock_deadline_and_reaps() {
        let dir = tempfile::tempdir().unwrap();
        let worker = fixture(
            dir.path(),
            r#"printf '%s' "$$" > "$0.pid"
while :; do :; done"#,
        );
        let result = supervise(
            &worker,
            Vec::new(),
            &AtomicBool::new(false),
            Duration::from_millis(250),
            None,
        );
        assert_eq!(result, Err(Failure::Deadline));
        let pid = fs::read_to_string(worker.with_extension("pid")).unwrap();
        assert!(!Path::new("/proc").join(pid).exists());
    }

    #[test]
    fn supervisor_cancels_and_reaps() {
        let dir = tempfile::tempdir().unwrap();
        let worker = fixture(
            dir.path(),
            r#"printf '%s' "$$" > "$0.pid"
while :; do :; done"#,
        );
        let cancelled = AtomicBool::new(false);
        let pid_file = worker.with_extension("pid");
        thread::scope(|scope| {
            let run = scope.spawn(|| supervise(&worker, Vec::new(), &cancelled, DEADLINE, None));
            let start = Instant::now();
            let pid = loop {
                if let Ok(pid) = fs::read_to_string(&pid_file) {
                    if !pid.is_empty() {
                        break pid;
                    }
                }
                assert!(start.elapsed() < DEADLINE);
                thread::sleep(CHECK_INTERVAL);
            };
            cancelled.store(true, Ordering::Relaxed);
            assert_eq!(run.join().unwrap(), Err(Failure::Cancelled));
            assert!(!Path::new("/proc").join(pid).exists());
        });
    }
}
