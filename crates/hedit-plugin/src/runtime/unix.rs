//! Unix pipe adapter and supervision; Lua and policy live in the shared engine.
#[cfg(test)]
use super::engine::DEADLINE;
use super::protocol::MAX_BATCH_REPLY_BYTES;
use super::HELPER_ARG;
use crate::Error;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::thread;
use std::time::{Duration, Instant};

const CHECK_INTERVAL: Duration = Duration::from_millis(10);

pub(super) fn executable() -> Result<PathBuf, Error> {
    crate::sandbox::worker_executable().map_err(|_| Error::Sandbox)
}

struct ReapChild(Child);

impl Drop for ReapChild {
    fn drop(&mut self) {
        // Also reap the child after a cancelled run or an IPC error.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn set_nonblocking(pipe: impl AsFd) -> Result<(), Error> {
    let fd = pipe.as_fd().as_raw_fd();
    // The pipe remains open while we change its status flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(Error::Worker);
    }
    Ok(())
}

pub(super) fn supervise(
    executable: &Path,
    request: Vec<u8>,
    cancelled: &AtomicBool,
    timeout: Duration,
    approval: Option<File>,
) -> Result<Vec<u8>, Error> {
    supervise_profiled(
        executable,
        request,
        cancelled,
        timeout,
        approval,
        &Default::default(),
    )
}
pub(super) fn supervise_profiled(
    executable: &Path,
    request: Vec<u8>,
    cancelled: &AtomicBool,
    timeout: Duration,
    approval: Option<File>,
    trace: &crate::telemetry::Recorder,
) -> Result<Vec<u8>, Error> {
    supervise_traced(
        executable,
        request,
        cancelled,
        timeout,
        approval,
        crate::sandbox::verify_child,
        trace,
    )
}

#[cfg(test)]
fn supervise_verified(
    executable: &Path,
    request: Vec<u8>,
    cancelled: &AtomicBool,
    timeout: Duration,
    approval: Option<File>,
    verify: impl FnOnce(&Child) -> io::Result<()>,
) -> Result<Vec<u8>, Error> {
    supervise_traced(
        executable,
        request,
        cancelled,
        timeout,
        approval,
        verify,
        &Default::default(),
    )
}
fn supervise_traced(
    executable: &Path,
    request: Vec<u8>,
    cancelled: &AtomicBool,
    timeout: Duration,
    mut approval: Option<File>,
    verify: impl FnOnce(&Child) -> io::Result<()>,
    trace: &crate::telemetry::Recorder,
) -> Result<Vec<u8>, Error> {
    use crate::telemetry::Phase;
    let started = Instant::now();
    let mut delivered = None;
    if cancelled.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
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
            .map_err(|_| Error::Worker)?,
    );
    verify(&child.0).map_err(|_| Error::Sandbox)?;
    trace.record(Phase::Startup, started.elapsed());
    let transfer_started = Instant::now();
    let input = child.0.stdin.take().ok_or(Error::Worker)?;
    let output = child.0.stdout.take().ok_or(Error::Worker)?;
    set_nonblocking(&input)?;
    set_nonblocking(&output)?;
    let mut input = Some(input);
    let output_fd = output.as_raw_fd();
    let mut output = output.take(MAX_BATCH_REPLY_BYTES as u64 + 2);
    let mut sent = 0;
    let mut reply = Vec::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Error::Deadline);
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
                Err(_) => return Err(Error::Worker),
            }
        }

        // Serialize revocation with delivery of the selection to the worker.
        // Once delivery ends, a revoke can proceed while computation runs.
        if input.is_none() {
            if delivered.is_none() {
                trace.record(Phase::Delivery, transfer_started.elapsed());
                delivered = Some(Instant::now());
            }
            drop(approval.take());
        }
        let mut status = child.0.try_wait().map_err(|_| Error::Worker)?;
        if status.is_none() {
            match crate::sandbox::over_memory_limit(&child.0) {
                Ok(true) => return Err(Error::Memory),
                Ok(false) => {}
                Err(_) => {
                    // A short-lived worker can exit between try_wait and the
                    // OS resource query. Accept that race only after reaping.
                    status = child.0.try_wait().map_err(|_| Error::Worker)?;
                    if status.is_none() {
                        return Err(Error::Worker);
                    }
                }
            }
        }
        let output_open = match output.read_to_end(&mut reply) {
            Ok(_) => false,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => true,
            Err(_) => return Err(Error::Worker),
        };
        if reply.len() > MAX_BATCH_REPLY_BYTES + 1 {
            return Err(Error::Worker);
        }
        if let Some(status) = status {
            if !status.success() {
                return Err(Error::Worker);
            }
            if !output_open {
                break;
            }
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

    if let Some(delivered) = delivered {
        trace.record(Phase::Transport, delivered.elapsed());
    }
    super::protocol::worker_reply(reply, sent == request.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn supervise(
        executable: &Path,
        request: Vec<u8>,
        cancelled: &AtomicBool,
        timeout: Duration,
        approval: Option<File>,
    ) -> Result<Vec<u8>, Error> {
        super::supervise_verified(
            executable,
            request,
            cancelled,
            timeout,
            approval,
            |_| Ok(()),
        )
    }

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
    fn verification_failure_delivers_no_text_and_reaps_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let worker = fixture(dir.path(), "exec /bin/cat > \"$0.received\"");
        let pid = std::cell::Cell::new(0);
        let result = super::supervise_verified(
            &worker,
            b"private fixture must not cross the boundary".to_vec(),
            &AtomicBool::new(false),
            DEADLINE,
            None,
            |child| {
                pid.set(child.id());
                Err(io::Error::other("rejected worker identity"))
            },
        );
        assert_eq!(result, Err(Error::Sandbox));
        assert!(fs::read(worker.with_extension("received"))
            .unwrap_or_default()
            .is_empty());
        assert_ne!(pid.get(), 0);
        assert_eq!(unsafe { libc::kill(pid.get() as i32, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn malformed_oversized_and_premature_replies_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for (script, request) in [
            ("exec /bin/cat >/dev/null", b"request".to_vec()),
            ("/bin/cat >/dev/null; printf '\\377'", b"request".to_vec()),
            (
                "/bin/cat >/dev/null; printf '\\007private fixture'",
                b"request".to_vec(),
            ),
            (
                "/bin/cat >/dev/null; printf '\\000[]'; exit 1",
                b"request".to_vec(),
            ),
            // Success cannot be accepted if the child exits before consuming
            // a request too large to fit in the pipe.
            ("printf '\\000[]'", vec![b'x'; 2 * 1024 * 1024]),
            (
                "/bin/cat >/dev/null; printf '\\000'; exec /usr/bin/yes x",
                b"request".to_vec(),
            ),
        ] {
            let worker = fixture(dir.path(), script);
            assert_eq!(
                supervise(&worker, request, &AtomicBool::new(false), DEADLINE, None),
                Err(Error::Worker),
                "{script}"
            );
        }
    }

    #[test]
    fn blocked_request_and_unterminated_success_remain_subject_to_deadlines() {
        let dir = tempfile::tempdir().unwrap();
        for script in [
            "while :; do :; done",
            "printf '\\000[]'; while :; do :; done",
        ] {
            let worker = fixture(dir.path(), script);
            assert_eq!(
                supervise(
                    &worker,
                    vec![b'x'; 2 * 1024 * 1024],
                    &AtomicBool::new(false),
                    Duration::from_millis(250),
                    None
                ),
                Err(Error::Deadline)
            );
        }
    }

    #[test]
    fn successful_exit_does_not_accept_a_reply_without_output_eof() {
        let dir = tempfile::tempdir().unwrap();
        // Model a compromised worker passing stdout to another process before
        // exiting. The bounded-lived fixture keeps the pipe open past the
        // deadline; process exit alone must not complete the response frame.
        let worker = fixture(
            dir.path(),
            "/bin/cat >/dev/null; /bin/sleep 1 &\nprintf '\\000[]'; exit 0",
        );
        assert_eq!(
            supervise(
                &worker,
                b"request".to_vec(),
                &AtomicBool::new(false),
                Duration::from_millis(250),
                None
            ),
            Err(Error::Deadline)
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
        assert_eq!(result, Err(Error::Deadline));
        let pid = fs::read_to_string(worker.with_extension("pid")).unwrap();
        assert_eq!(unsafe { libc::kill(pid.trim().parse().unwrap(), 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
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
            assert_eq!(run.join().unwrap(), Err(Error::Cancelled));
            assert_eq!(unsafe { libc::kill(pid.trim().parse().unwrap(), 0) }, -1);
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        });
    }
}
