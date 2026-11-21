use super::{nonblocking, Process};
use crate::tool::{
    stream::{check, Progress, Rate, CHUNK},
    Launch, HEADER_BYTES, READY, STDERR_BYTES,
};
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    process::{ChildStderr, ChildStdin, ChildStdout},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub(in crate::tool) struct Stream {
    child: Process,
    input: ChildStdin,
    output: ChildStdout,
    stderr: ChildStderr,
    errors: Rate,
    cpu: Duration,
    error_eof: bool,
    accounting: Instant,
}
fn write(pipe: &mut impl Write, bytes: &[u8]) -> io::Result<usize> {
    if bytes.is_empty() {
        return Ok(0);
    }
    match pipe.write(bytes) {
        Ok(0) => Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "Native service input closed.",
        )),
        Ok(count) => Ok(count),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(0)
        }
        Err(error) => Err(error),
    }
}
fn read(pipe: &mut impl Read, bytes: &mut [u8]) -> io::Result<usize> {
    match pipe.read(bytes) {
        Ok(0) => Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Native service output closed.",
        )),
        Ok(count) => Ok(count),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(0)
        }
        Err(error) => Err(error),
    }
}
impl Stream {
    pub(in crate::tool) fn open(
        launch: &Launch,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Self> {
        let header = serde_json::to_vec(launch).map_err(|_| crate::tool::invalid())?;
        if header.len() > HEADER_BYTES {
            return Err(crate::tool::invalid());
        }
        let mut bytes = (header.len() as u32).to_le_bytes().to_vec();
        bytes.extend(header);
        let mut child = Process::spawn(
            &crate::tool::launcher()?,
            cancel,
            deadline,
            crate::tool::Role::Service,
        )?;
        let input = child.child.stdin.take().ok_or_else(crate::tool::invalid)?;
        let output = child.child.stdout.take().ok_or_else(crate::tool::invalid)?;
        let stderr = child.child.stderr.take().ok_or_else(crate::tool::invalid)?;
        nonblocking(&input)?;
        nonblocking(&output)?;
        nonblocking(&stderr)?;
        let mut stream = Self {
            child,
            input,
            output,
            stderr,
            errors: Rate::new(Instant::now(), STDERR_BYTES as u64),
            cpu: Duration::ZERO,
            error_eof: false,
            accounting: Instant::now(),
        };
        let mut sent = 0;
        let mut received = 0;
        let mut ready = [0; READY.len()];
        while received < ready.len() {
            check(cancel, deadline)?;
            stream.resources()?;
            sent += write(&mut stream.input, &bytes[sent..])?;
            received += read(&mut stream.output, &mut ready[received..])?;
            stream.drain_errors()?;
            if received < ready.len() {
                stream.wait(sent < bytes.len())?;
            }
        }
        if ready != *READY || sent != bytes.len() {
            return Err(io::Error::other(
                "Native service confinement acknowledgement failed.",
            ));
        }
        Ok(stream)
    }
    fn resources(&mut self) -> io::Result<()> {
        if self.child.status()?.is_some() {
            return Err(io::Error::other("Native service exited."));
        }
        if self.accounting.elapsed() >= Duration::from_millis(50) {
            self.cpu = crate::tool::native::check_resources(self.child.pid)?;
            self.accounting = Instant::now();
        }
        Ok(())
    }
    fn drain_errors(&mut self) -> io::Result<()> {
        if self.error_eof {
            return Ok(());
        }
        let mut bytes = [0; CHUNK];
        match self.stderr.read(&mut bytes) {
            Ok(count) => {
                self.error_eof = count == 0;
                self.errors.charge(Instant::now(), count as u64)?;
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
    pub(in crate::tool) fn cpu_time(&self) -> Duration {
        self.cpu
    }
    pub(in crate::tool) fn poll(
        &mut self,
        input: &[u8],
        output: &mut [u8; CHUNK],
    ) -> io::Result<Progress> {
        self.resources()?;
        self.drain_errors()?;
        let read = read(&mut self.output, output)?;
        let written = write(&mut self.input, input)?;
        Ok(Progress { written, read })
    }
    pub(in crate::tool) fn wait(&self, input_pending: bool) -> io::Result<()> {
        let mut descriptors = [
            libc::pollfd {
                fd: self.output.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if self.error_eof {
                    -1
                } else {
                    self.stderr.as_raw_fd()
                },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if input_pending {
                    self.input.as_raw_fd()
                } else {
                    -1
                },
                events: libc::POLLOUT,
                revents: 0,
            },
        ];
        let result = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, 50) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        Ok(())
    }
}
