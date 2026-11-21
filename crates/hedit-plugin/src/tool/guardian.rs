//! macOS has no parent-death signal. A trusted, event-driven guardian watches the
//! native process and terminates it when the editor's control socket closes.
use super::HELPER;
use std::{
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub(super) const ARG: &str = "--hedit-tool-guardian";
const CONTROL: i32 = 3;
const ACK: &[u8; 4] = b"HG01";

pub(super) struct Process {
    pub child: Child,
    pub pid: u32,
    control: Option<UnixStream>,
    finished: Option<ExitStatus>,
}
impl Process {
    pub fn spawn(launcher: &Path, cancel: &AtomicBool, deadline: Instant) -> io::Result<Self> {
        let (control, inherited) = UnixStream::pair()?;
        control.set_nonblocking(true)?;
        let descriptor = inherited.as_raw_fd();
        let mut command = Command::new(launcher);
        command
            .arg(HELPER)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Only async-signal-safe operations run between fork and exec. The
        // native process's group ID remains reserved until this Child is reaped.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0
                    || libc::dup2(descriptor, CONTROL) < 0
                    || libc::fcntl(CONTROL, libc::F_SETFD, 0) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop(inherited);
        let mut process = Self {
            child,
            pid: 0,
            control: Some(control),
            finished: None,
        };
        let mut ack = [0; 8];
        let mut read = 0;
        while read < ack.len() {
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Tool cancelled.",
                ));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "Tool timed out."));
            }
            match process.control.as_mut().unwrap().read(&mut ack[read..]) {
                Ok(0) => return Err(io::Error::other("Tool guardian disconnected.")),
                Ok(count) => read += count,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => return Err(error),
            }
        }
        process.pid = u32::from_le_bytes(ack[4..].try_into().unwrap());
        if &ack[..4] != ACK || process.pid != process.child.id() {
            return Err(io::Error::other("Tool guardian acknowledgement failed."));
        }
        Ok(process)
    }

    // Observe without reaping: even an abnormal guardian exit must leave its
    // PID reserved until the complete process group has been terminated.
    pub fn status(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.finished.is_some() {
            return Ok(self.finished);
        }
        let mut information = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                information.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { information.assume_init() }.si_pid == 0 {
            if let Some(control) = self.control.as_mut() {
                let mut byte = [0];
                match control.read(&mut byte) {
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) =>
                    {
                        return Ok(None)
                    }
                    _ => {}
                }
                // Loss of the trusted guardian also ends native execution.
                unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
                let status = self.child.wait()?;
                self.finished = Some(status);
                return Ok(Some(status));
            }
            return Ok(None);
        }
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let status = self.child.wait()?;
        self.finished = Some(status);
        Ok(Some(status))
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.control.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.status() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => break,
            }
        }
        // The Child has not been reaped, so this group ID cannot have become
        // an unrelated process's ID. Also cover a wedged or crashed guardian.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

fn event(identifier: usize, filter: i16, notes: u32) -> libc::kevent {
    libc::kevent {
        ident: identifier,
        filter,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
        fflags: notes,
        data: 0,
        udata: std::ptr::null_mut(),
    }
}
fn socket(fd: i32) -> io::Result<UnixStream> {
    let mut kind = 0i32;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut i32).cast(),
            &mut length,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return Err(io::Error::other("Tool guardian control is unavailable."));
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { UnixStream::from_raw_fd(fd) })
}

// Arm before native::prepare closes descriptors and prohibits process creation.
// The native bootstrap is a session leader. Its trusted guardian inherits that
// group, anchoring the group ID until cleanup, including after editor death.
pub(super) fn arm() -> io::Result<()> {
    let control = socket(CONTROL)?;
    let (mut ready, inherited) = UnixStream::pair()?;
    ready.set_nonblocking(true)?;
    let descriptor = inherited.as_raw_fd();
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg(ARG)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(descriptor, 4) < 0
                || libc::fcntl(CONTROL, libc::F_SETFD, 0) < 0
                || libc::fcntl(4, libc::F_SETFD, 0) < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut guardian = command.spawn()?;
    drop(inherited);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ack = [0; 4];
    let mut read = 0;
    let result = loop {
        if Instant::now() >= deadline {
            break Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Tool guardian timed out.",
            ));
        }
        match ready.read(&mut ack[read..]) {
            Ok(0) => break Err(io::Error::other("Tool guardian disconnected.")),
            Ok(count) => {
                read += count;
                if read == ack.len() {
                    break if &ack == ACK {
                        Ok(())
                    } else {
                        Err(io::Error::other("Tool guardian acknowledgement failed."))
                    };
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(error) => break Err(error),
        }
    };
    if result.is_err() {
        let _ = guardian.kill();
        let _ = guardian.wait();
    }
    drop(control);
    // Child::drop does not terminate the guardian; it survives native exec.
    result
}

struct Terminate(i32);
impl Drop for Terminate {
    fn drop(&mut self) {
        unsafe { libc::kill(self.0, libc::SIGKILL) };
    }
}
pub(super) fn run() -> io::Result<bool> {
    crate::sandbox::close_guardian_descriptors()?;
    let parent = unsafe { libc::getppid() };
    if parent <= 1 || unsafe { libc::getpgrp() } != parent || unsafe { libc::getsid(0) } != parent {
        return Err(io::Error::other("Tool guardian session is unavailable."));
    }
    // This trusted process remains in the native session leader's group. The
    // OS cannot reuse that group ID while this process is alive, even if the
    // editor and native process exit before the guardian handles its event.
    let mut control = socket(CONTROL)?;
    let mut ready = socket(4)?;
    let _terminate = Terminate(parent);
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        return Err(io::Error::last_os_error());
    }
    let queue = unsafe { OwnedFd::from_raw_fd(queue) };
    let changes = [
        event(CONTROL as usize, libc::EVFILT_READ, 0),
        event(parent as usize, libc::EVFILT_PROC, libc::NOTE_EXIT),
    ];
    if unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            changes.as_ptr(),
            changes.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut ack = ACK.to_vec();
    ack.extend_from_slice(&(parent as u32).to_le_bytes());
    control.write_all(&ack)?;
    ready.write_all(ACK)?;
    drop(ready);
    loop {
        let mut ready = [std::mem::MaybeUninit::<libc::kevent>::uninit(); 2];
        let count = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                std::ptr::null(),
                0,
                ready.as_mut_ptr().cast(),
                ready.len() as i32,
                std::ptr::null(),
            )
        };
        if count < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io::Error::last_os_error());
        }
        for ready in &ready[..count as usize] {
            let ready = unsafe { ready.assume_init_ref() };
            if ready.flags & libc::EV_ERROR != 0 {
                return Err(io::Error::other("Tool guardian event failed."));
            }
            if ready.ident == CONTROL as usize && ready.filter == libc::EVFILT_READ {
                return Ok(false);
            }
            if ready.ident == parent as usize && ready.filter == libc::EVFILT_PROC {
                return Ok(true);
            }
        }
    }
}
