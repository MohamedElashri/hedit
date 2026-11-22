use super::{Pipe, Worker};
use crate::tool::{
    stream::{check, Progress, Rate, CHUNK},
    Launch, STDERR_BYTES,
};
use std::{
    io,
    os::windows::io::AsRawHandle,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::WaitForMultipleObjects,
};

pub(in crate::tool) struct Stream {
    child: Worker,
    input: Pipe,
    output: Pipe,
    stderr: Pipe,
    errors: Vec<u8>,
    error_rate: Rate,
    cpu: Duration,
    error_eof: bool,
    accounting: Instant,
}
impl Stream {
    pub(in crate::tool) fn open(
        launch: &Launch,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Self> {
        check(cancel, deadline)?;
        let mut child = Worker::spawn_native(launch, crate::tool::Role::Service)?;
        let input = Pipe::new(child.input.take().ok_or_else(crate::tool::invalid)?)?;
        let output = Pipe::new(child.output.take().ok_or_else(crate::tool::invalid)?)?;
        let stderr = Pipe::new(child.stderr.take().ok_or_else(crate::tool::invalid)?)?;
        check(cancel, deadline)?;
        Ok(Self {
            child,
            input,
            output,
            stderr,
            errors: Vec::new(),
            error_rate: Rate::new(Instant::now(), STDERR_BYTES as u64),
            cpu: Duration::ZERO,
            error_eof: false,
            accounting: Instant::now(),
        })
    }
    pub(in crate::tool) fn cpu_time(&self) -> Duration {
        self.cpu
    }
    pub(in crate::tool) fn poll(
        &mut self,
        input: &[u8],
        output: &mut [u8; CHUNK],
    ) -> io::Result<Progress> {
        if self.child.finished(0)? {
            return Err(io::Error::other("Native service exited."));
        }
        if self.accounting.elapsed() >= Duration::from_millis(50) {
            self.cpu = self.child.cpu_time()?;
            if self.child.over_thread_limit()? {
                return Err(io::Error::other("Tool thread limit exceeded."));
            }
            self.accounting = Instant::now();
        }
        self.errors.clear();
        self.stderr
            .read(&mut self.errors, STDERR_BYTES, &mut self.error_eof)?;
        self.error_rate
            .charge(Instant::now(), self.errors.len() as u64)?;
        let read = match self.output.transfer(None)? {
            Some(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Native service output closed.",
                ))
            }
            Some(count) => {
                output[..count].copy_from_slice(&self.output.bytes[..count]);
                count
            }
            None => 0,
        };
        let written = if input.is_empty() {
            0
        } else {
            self.input.transfer(Some(input))?.unwrap_or(0)
        };
        if !input.is_empty() && written == 0 && !self.input.pending {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "Native service input closed.",
            ));
        }
        Ok(Progress { written, read })
    }
    pub(in crate::tool) fn wait(&self, input_pending: bool) -> io::Result<()> {
        let mut handles = [std::ptr::null_mut(); 3];
        let mut count = 0;
        for (pipe, enabled) in [
            (&self.input, input_pending),
            (&self.output, true),
            (&self.stderr, !self.error_eof),
        ] {
            if enabled && pipe.pending {
                handles[count] = pipe.event.as_raw_handle();
                count += 1;
            }
        }
        if count == 0 {
            std::thread::sleep(Duration::from_millis(50));
            return Ok(());
        }
        let status = unsafe { WaitForMultipleObjects(count as u32, handles.as_ptr(), 0, 50) };
        if status == WAIT_TIMEOUT || (WAIT_OBJECT_0..WAIT_OBJECT_0 + count as u32).contains(&status)
        {
            Ok(())
        } else if status == WAIT_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Err(io::Error::other("Native service wait failed."))
        }
    }
}
