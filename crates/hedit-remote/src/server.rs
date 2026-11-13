//! Bounded socket peers feed one document/storage actor. Disconnect cannot cancel
//! a request already handed to that actor.
use crate::{
    host,
    journal::Journal,
    protocol::{self, Envelope, Error, Reply, Request, Response, Result, VERSION},
};
use std::{
    fs,
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const PEERS: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(15);
enum Message {
    Request(u64, Box<Envelope>, SyncSender<Reply>),
    Disconnect(u64),
}
struct PeerCount(Arc<AtomicUsize>);
impl Drop for PeerCount {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn peer(mut stream: UnixStream, connection: u64, tx: SyncSender<Message>, count: Arc<AtomicUsize>) {
    let _guard = PeerCount(count);
    let result = (|| -> io::Result<()> {
        host::verify_peer(&stream).map_err(io::Error::other)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        let mut hello = false;
        while let Some(envelope) = protocol::read_frame::<Envelope>(&mut stream)? {
            if envelope.version != VERSION
                || (!hello && !matches!(envelope.request, Request::Hello {}))
            {
                protocol::write_frame(
                    &mut stream,
                    &Reply {
                        version: VERSION,
                        id: envelope.id,
                        result: Err(Error::Protocol),
                    },
                )?;
                break;
            }
            hello = true;
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.send(Message::Request(connection, Box::new(envelope), reply_tx))
                .map_err(io::Error::other)?;
            let reply = reply_rx.recv().map_err(io::Error::other)?;
            protocol::write_frame(&mut stream, &reply)?;
        }
        Ok(())
    })();
    let _ = result; // No remote payload or raw IO diagnostic is logged.
    let _ = tx.send(Message::Disconnect(connection));
}

fn socket_metadata(path: &Path) -> Result<()> {
    let m = path.symlink_metadata().map_err(|_| Error::Unavailable)?;
    if !m.file_type().is_socket()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o077 != 0
    {
        return Err(Error::Permission);
    }
    Ok(())
}

pub fn run(state: &Path, idle: Duration) -> Result<()> {
    let mut journal = Journal::open(state)?;
    let directory = host::private_directory(state)?;
    let mut pid = host::private_file(&directory, "worker.pid")?;
    pid.set_len(0).map_err(|_| Error::Storage)?;
    writeln!(pid, "{}", std::process::id()).map_err(|_| Error::Storage)?;
    let socket = state.join("session.sock");
    if socket.symlink_metadata().is_ok() {
        socket_metadata(&socket)?;
        if UnixStream::connect(&socket).is_ok() {
            return Err(Error::Busy);
        }
        fs::remove_file(&socket).map_err(|_| Error::Permission)?;
    }
    let listener = UnixListener::bind(&socket).map_err(|_| Error::Unavailable)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .map_err(|_| Error::Permission)?;
    let (tx, rx) = mpsc::sync_channel(32);
    let active = Arc::new(AtomicUsize::new(0));
    let listener_active = active.clone();
    thread::spawn(move || {
        let next = AtomicU64::new(1);
        loop {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    // Descriptor/resource exhaustion must not become a hot loop.
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            if listener_active.load(Ordering::SeqCst) >= PEERS {
                continue;
            }
            listener_active.fetch_add(1, Ordering::SeqCst);
            let (count, tx) = (listener_active.clone(), tx.clone());
            let id = next.fetch_add(1, Ordering::SeqCst);
            if thread::Builder::new()
                .spawn(move || peer(stream, id, tx, count))
                .is_err()
            {
                listener_active.fetch_sub(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(100));
            }
        }
    });
    let mut owner: Option<(u64, u64)> = None;
    let mut runtime = crate::runtime::Runtime::default();
    let mut last = Instant::now();
    loop {
        runtime.completed(&mut journal);
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Message::Disconnect(id)) => {
                runtime.disconnected(id);
                if owner.is_some_and(|(connection, _)| connection == id) {
                    owner = None;
                }
                last = Instant::now();
            }
            Ok(Message::Request(connection, envelope, reply_tx)) => {
                let request = match envelope.request {
                    Request::UploadRun { id } => runtime.uploaded(connection, id),
                    request => Ok(request),
                };
                let result = request.and_then(|request| {
                    dispatch(&mut journal, &mut owner, &mut runtime, connection, request)
                });
                let result = runtime.reply(connection, result);
                let _ = reply_tx.send(Reply {
                    version: VERSION,
                    id: envelope.id,
                    result,
                });
                last = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if active.load(Ordering::SeqCst) == 0 && last.elapsed() >= idle {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    journal.checkpoint()?;
    fs::remove_file(socket).map_err(|_| Error::Storage)?;
    Ok(())
}

fn dispatch(
    journal: &mut Journal,
    owner: &mut Option<(u64, u64)>,
    runtime: &mut crate::runtime::Runtime,
    connection: u64,
    request: Request,
) -> Result<Response> {
    if let Some(result) = runtime.handle(journal, connection, &request) {
        return result;
    }
    match request {
        Request::Hello {} => Ok(Response::Hello {
            session: journal.session().to_owned(),
            worker_pid: std::process::id(),
            version: VERSION,
            max_frame: protocol::MAX_FRAME,
        }),
        Request::Ping {} => Ok(Response::Pong {}),
        Request::Workspace { path } => {
            if !runtime.ready_for_owner() {
                return Err(Error::Busy);
            }
            journal.workspace(&path)?;
            Ok(Response::Workspace {})
        }
        Request::Acquire { takeover } => {
            if let Some((prior, epoch)) = owner {
                if *prior == connection {
                    return Ok(Response::Owner {
                        epoch: *epoch,
                        token: runtime.token.clone(),
                    });
                }
                if !takeover {
                    return Err(Error::Busy);
                }
            }
            // This actor cannot grant ownership in the middle of an edit commit.
            runtime.completed(journal);
            if !runtime.ready_for_owner() {
                return Err(Error::Busy);
            }
            let epoch = journal.acquire()?;
            runtime.owner(connection, epoch)?;
            *owner = Some((connection, epoch));
            Ok(Response::Owner {
                epoch,
                token: runtime.token.clone(),
            })
        }
        Request::Open { epoch, path } => {
            authorize(*owner, connection, epoch)?;
            Ok(Response::Document {
                info: journal.open_document(&path)?,
            })
        }
        Request::List {} => Ok(Response::Documents {
            items: journal.list()?,
        }),
        Request::Read { document, offset } => {
            let (info, text, eof) = journal.read(document, offset)?;
            Ok(Response::Page {
                info,
                offset,
                text,
                eof,
            })
        }
        Request::Apply {
            session,
            epoch,
            document,
            sequence,
            base_revision,
            action,
        } => {
            authorize(*owner, connection, epoch)?;
            if session != journal.session() {
                return Err(Error::Conflict);
            }
            Ok(Response::Durable {
                info: journal.apply(document, sequence, base_revision, &action)?,
                sequence,
            })
        }
        _ => Err(Error::Unsupported),
    }
}
fn authorize(owner: Option<(u64, u64)>, connection: u64, epoch: u64) -> Result<()> {
    if owner != Some((connection, epoch)) {
        return Err(Error::Fenced);
    }
    Ok(())
}

fn connect(state: &Path) -> Result<UnixStream> {
    let socket = state.join("session.sock");
    socket_metadata(&socket)?;
    let stream = UnixStream::connect(socket).map_err(|_| Error::Unavailable)?;
    host::verify_peer(&stream)?;
    Ok(stream)
}

pub fn bridge(state: &Path) -> Result<()> {
    let dir = host::private_directory(state)?;
    let mut stream = match connect(state) {
        Ok(stream) => stream,
        Err(Error::Permission) => return Err(Error::Permission),
        Err(_) => {
            let startup = host::private_file(&dir, "startup.lock")?;
            startup.try_lock().map_err(|_| Error::Busy)?;
            if let Ok(stream) = connect(state) {
                stream
            } else {
                let lock = host::private_file(&dir, "writer.lock")?;
                lock.try_lock().map_err(|_| Error::Busy)?;
                drop(lock);
                let exe = std::env::current_exe().map_err(|_| Error::Unavailable)?;
                let mut command = Command::new(exe);
                command
                    .arg("worker")
                    .arg("--state")
                    .arg(state)
                    .current_dir("/")
                    .env_clear()
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                // SAFETY: only async-signal-safe libc functions run after fork.
                unsafe {
                    command.pre_exec(|| {
                        if libc::setsid() == -1 {
                            return Err(io::Error::last_os_error());
                        }
                        libc::umask(0o077);
                        Ok(())
                    });
                }
                let mut child = command.spawn().map_err(|_| Error::Unavailable)?;
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    if let Ok(stream) = connect(state) {
                        break stream;
                    }
                    if child.try_wait().map_err(|_| Error::Unavailable)?.is_some() {
                        return Err(Error::Unavailable);
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(Error::Unavailable);
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    };
    let mut input_stream = stream.try_clone().map_err(|_| Error::Unavailable)?;
    thread::spawn(move || {
        let _ = io::copy(&mut io::stdin().lock(), &mut input_stream);
        let _ = input_stream.shutdown(Shutdown::Write);
    });
    // Stdout is line-buffered even with a binary protocol. io::copy alone can
    // hold a small reply until EOF; flush each bounded chunk explicitly.
    let mut out = io::stdout().lock();
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Error::Unavailable),
        };
        out.write_all(&buffer[..count])
            .and_then(|_| out.flush())
            .map_err(|_| Error::Unavailable)?;
    }
    Ok(())
}
