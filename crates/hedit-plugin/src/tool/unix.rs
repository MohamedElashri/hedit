use super::{Launch, Output, HEADER_BYTES, OUTPUT_BYTES, READY, STDERR_BYTES};
use std::{
    io::{self, Read, Write},
    os::fd::{AsFd, AsRawFd},
    path::Path,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use super::HELPER;

#[cfg(target_os = "linux")]
struct Process {
    child: std::process::Child,
    pid: u32,
}
#[cfg(target_os = "linux")]
impl Process {
    fn spawn(launcher: &Path, _: &AtomicBool, _: Instant) -> io::Result<Self> {
        use std::process::Stdio;
        let child = Command::new(launcher)
            .arg(HELPER)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let pid = child.id();
        Ok(Self { child, pid })
    }
    fn status(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }
}
#[cfg(target_os = "linux")]
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[cfg(target_os = "macos")]
use super::guardian::Process;
fn nonblocking(pipe: &impl AsFd) -> io::Result<()> {
    let fd = pipe.as_fd().as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn read(
    pipe: &mut impl Read,
    bytes: &mut Vec<u8>,
    maximum: usize,
    eof: &mut bool,
) -> io::Result<()> {
    if *eof {
        return Ok(());
    }
    let mut buffer = [0; 4096];
    match pipe.read(&mut buffer) {
        Ok(0) => *eof = true,
        Ok(count) => {
            if count > maximum.saturating_sub(bytes.len()) {
                return Err(io::Error::other("Tool output limit exceeded."));
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) => {}
        Err(error) => return Err(error),
    }
    Ok(())
}
pub(super) fn run(
    launcher: &Path,
    launch: &Launch,
    input: &[u8],
    cancel: &AtomicBool,
    timeout: Duration,
) -> io::Result<Output> {
    let started = Instant::now();
    let header = serde_json::to_vec(launch).map_err(|_| super::invalid())?;
    if header.len() > HEADER_BYTES {
        return Err(super::invalid());
    }
    let mut request = (header.len() as u32).to_le_bytes().to_vec();
    request.extend_from_slice(&header);
    if cancel.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Tool cancelled.",
        ));
    }
    let mut child = Process::spawn(launcher, cancel, started + timeout)?;
    let mut stdin = Some(child.child.stdin.take().ok_or_else(super::invalid)?);
    let mut stdout = child.child.stdout.take().ok_or_else(super::invalid)?;
    let mut stderr = child.child.stderr.take().ok_or_else(super::invalid)?;
    nonblocking(stdin.as_ref().unwrap())?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let (mut sent, mut ready, mut out_eof, mut err_eof) = (0, false, false, false);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    loop {
        let previous = (sent, ready, out_eof, err_eof, out.len(), err.len());
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Tool cancelled.",
            ));
        }
        if started.elapsed() >= timeout {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Tool timed out."));
        }
        if child.status()?.is_none() {
            match super::native::check_resources(child.pid) {
                Ok(()) => {}
                Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {}
                Err(error) if child.status()?.is_none() => return Err(error),
                Err(_) => {}
            }
        }
        read(
            &mut stdout,
            &mut out,
            OUTPUT_BYTES + if ready { 0 } else { READY.len() },
            &mut out_eof,
        )?;
        read(&mut stderr, &mut err, STDERR_BYTES, &mut err_eof)?;
        if !ready && out.len() >= READY.len() {
            if &out[..READY.len()] != READY {
                return Err(io::Error::other("Tool confinement acknowledgement failed."));
            }
            out.drain(..READY.len());
            ready = true;
        }
        let bytes = if ready && sent >= request.len() {
            input
        } else {
            &request
        };
        let offset = if ready && sent >= request.len() {
            sent - request.len()
        } else {
            sent
        };
        if let Some(pipe) = stdin.as_mut() {
            if offset < bytes.len() {
                match pipe.write(&bytes[offset..]) {
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "Tool input closed.",
                        ))
                    }
                    Ok(count) => sent += count,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
            if ready && sent == request.len() + input.len() {
                stdin.take();
            }
        }
        if let Some(status) = child.status()? {
            if !status.success() {
                return Err(io::Error::other("Isolated tool failed."));
            }
            if out_eof && err_eof {
                if !ready || sent != request.len() + input.len() {
                    return Err(io::Error::other("Tool input was not delivered."));
                }
                return Ok(Output {
                    stdout: out,
                    stderr: err,
                });
            }
        }
        if previous == (sent, ready, out_eof, err_eof, out.len(), err.len()) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

pub(super) fn bootstrap() -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    #[cfg(target_os = "macos")]
    super::guardian::arm()?;
    super::native::prepare()?;
    for (fd, mode) in [
        (0, libc::O_RDONLY),
        (1, libc::O_WRONLY),
        (2, libc::O_WRONLY),
    ] {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0
            || unsafe { stat.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO
            || unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_ACCMODE != mode
        {
            return Err(io::Error::other("Tool transport is unavailable."));
        }
    }
    let mut input = io::stdin().lock();
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > HEADER_BYTES {
        return Err(super::invalid());
    }
    let mut header = vec![0; length];
    input.read_exact(&mut header)?;
    let launch: Launch = serde_json::from_slice(&header).map_err(|_| super::invalid())?;
    launch.validate()?;
    let mut command = Command::new(&launch.executable);
    command
        .args(&launch.arguments)
        .env_clear()
        .current_dir(&launch.snapshot);
    super::native::enter(&launch)?;
    io::stdout().write_all(READY)?;
    io::stdout().flush()?;
    drop(input);
    Err(command.exec())
}
