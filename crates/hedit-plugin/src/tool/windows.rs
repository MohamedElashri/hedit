use super::{Launch, Output, OUTPUT_BYTES, STDERR_BYTES};
use crate::sandbox::windows::Worker;
use std::{
    fs::File,
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{Threading::*, IO::*},
};

mod stream;
pub(super) use stream::Stream;

struct Pipe {
    file: File,
    event: OwnedHandle,
    operation: Box<OVERLAPPED>,
    bytes: Box<[u8; 4096]>,
    pending: bool,
}
impl Pipe {
    fn new(file: File) -> io::Result<Self> {
        let handle = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            file,
            event: unsafe { OwnedHandle::from_raw_handle(handle) },
            operation: Box::default(),
            bytes: Box::new([0; 4096]),
            pending: false,
        })
    }
    fn transfer(&mut self, input: Option<&[u8]>) -> io::Result<Option<usize>> {
        let handle = self.file.as_raw_handle();
        if !self.pending {
            if unsafe { ResetEvent(self.event.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            *self.operation = OVERLAPPED {
                hEvent: self.event.as_raw_handle(),
                ..Default::default()
            };
            let result = if let Some(input) = input {
                let count = input.len().min(self.bytes.len());
                self.bytes[..count].copy_from_slice(&input[..count]);
                unsafe {
                    WriteFile(
                        handle,
                        self.bytes.as_ptr(),
                        count as u32,
                        ptr::null_mut(),
                        &mut *self.operation,
                    )
                }
            } else {
                unsafe {
                    ReadFile(
                        handle,
                        self.bytes.as_mut_ptr(),
                        self.bytes.len() as u32,
                        ptr::null_mut(),
                        &mut *self.operation,
                    )
                }
            };
            if result == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
                return match unsafe { GetLastError() } {
                    ERROR_BROKEN_PIPE | ERROR_NO_DATA if input.is_none() => Ok(Some(0)),
                    error => Err(io::Error::from_raw_os_error(error as i32)),
                };
            }
            self.pending = true;
        }
        let mut count = 0;
        if unsafe { GetOverlappedResult(handle, &*self.operation, &mut count, 0) } == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_IO_INCOMPLETE {
                return Ok(None);
            }
            self.pending = false;
            return match error {
                ERROR_BROKEN_PIPE | ERROR_NO_DATA if input.is_none() => Ok(Some(0)),
                _ => Err(io::Error::from_raw_os_error(error as i32)),
            };
        }
        self.pending = false;
        if count as usize > self.bytes.len() {
            return Err(io::Error::other("Invalid tool pipe completion."));
        }
        Ok(Some(count as usize))
    }
    fn read(&mut self, result: &mut Vec<u8>, limit: usize, eof: &mut bool) -> io::Result<()> {
        if *eof {
            return Ok(());
        }
        if let Some(count) = self.transfer(None)? {
            if count == 0 {
                *eof = true;
            } else {
                if count > limit.saturating_sub(result.len()) {
                    return Err(io::Error::other("Tool output limit exceeded."));
                }
                result.extend_from_slice(&self.bytes[..count]);
            }
        }
        Ok(())
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        if self.pending {
            // Keep the event, OVERLAPPED and buffer alive until the cancelled
            // local pipe operation completes; never abandon borrowed I/O memory.
            unsafe { CancelIoEx(self.file.as_raw_handle(), &*self.operation) };
            let mut bytes = 0;
            unsafe {
                GetOverlappedResult(self.file.as_raw_handle(), &*self.operation, &mut bytes, 1)
            };
        }
    }
}
pub(super) fn run(
    launch: &Launch,
    input: &[u8],
    cancel: &AtomicBool,
    timeout: Duration,
) -> io::Result<Output> {
    let started = Instant::now();
    if cancel.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Tool cancelled.",
        ));
    }
    let mut child = Worker::spawn_native(launch)?;
    let mut stdin = Some(Pipe::new(child.input.take().ok_or_else(super::invalid)?)?);
    let mut stdout = Pipe::new(child.output.take().ok_or_else(super::invalid)?)?;
    let mut stderr = Pipe::new(child.stderr.take().ok_or_else(super::invalid)?)?;
    let result = (|| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (mut sent, mut out_eof, mut err_eof) = (0, false, false);
        let mut accounting = started;
        loop {
            let previous = (sent, out_eof, err_eof, out.len(), err.len());
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Tool cancelled.",
                ));
            }
            if started.elapsed() >= timeout {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "Tool timed out."));
            }
            if accounting.elapsed() >= Duration::from_millis(50) {
                if !child.finished(0)? {
                    if child.over_cpu_limit()? {
                        return Err(io::Error::other("Tool CPU limit exceeded."));
                    }
                    if child.over_thread_limit()? {
                        return Err(io::Error::other("Tool thread limit exceeded."));
                    }
                }
                accounting = Instant::now();
            }
            stdout.read(&mut out, OUTPUT_BYTES, &mut out_eof)?;
            stderr.read(&mut err, STDERR_BYTES, &mut err_eof)?;
            if let Some(pipe) = stdin.as_mut() {
                if sent < input.len() {
                    if let Some(count) = pipe.transfer(Some(&input[sent..]))? {
                        if count == 0 {
                            return Err(io::Error::new(
                                io::ErrorKind::WriteZero,
                                "Tool input closed.",
                            ));
                        }
                        sent += count;
                    }
                }
                if sent == input.len() {
                    stdin.take();
                }
            }
            if child.finished(0)? && out_eof && err_eof {
                if !child.success()? {
                    return Err(io::Error::other(format!(
                        "Isolated tool exited with status {:#010x}: {:?}",
                        child.exit_code()?,
                        String::from_utf8_lossy(&err)
                    )));
                }
                if sent != input.len() {
                    return Err(io::Error::other("Tool input was not delivered."));
                }
                return Ok(Output {
                    stdout: out,
                    stderr: err,
                });
            }
            if previous == (sent, out_eof, err_eof, out.len(), err.len()) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    })();
    child.terminate();
    result
}
