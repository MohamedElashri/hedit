//! Windows pipe/process adapter; the protocol and Lua engine are shared.
use super::{
    protocol::{self, MAX_BATCH_REPLY_BYTES},
    HELPER_ARG,
};
use crate::{sandbox::windows::Worker, Error};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) fn executable() -> Result<PathBuf, Error> {
    crate::sandbox::worker_executable().map_err(|_| Error::Sandbox)
}

pub(super) fn supervise(
    executable: &Path,
    request: Vec<u8>,
    cancelled: &AtomicBool,
    timeout: Duration,
    approval: Option<File>,
) -> Result<Vec<u8>, Error> {
    supervise_arguments(
        executable,
        &[HELPER_ARG],
        request,
        cancelled,
        timeout,
        approval,
    )
}

fn supervise_arguments(
    executable: &Path,
    arguments: &[&str],
    request: Vec<u8>,
    cancelled: &AtomicBool,
    timeout: Duration,
    approval: Option<File>,
) -> Result<Vec<u8>, Error> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let deadline = Instant::now() + timeout;
    let mut child = Worker::spawn(executable, arguments).map_err(|_| Error::Sandbox)?;
    let mut input = child.input.take().ok_or(Error::Worker)?;
    let output = child.output.take().ok_or(Error::Worker)?;
    thread::scope(|scope| {
        // Anonymous Windows pipes block. Dedicated scoped I/O threads keep the
        // supervisor responsive. Killing the single-process job closes the peer
        // handles and unblocks both threads before we join them.
        let writer = scope.spawn(move || {
            let delivered = input.write_all(&request).is_ok();
            drop(input);
            drop(approval);
            delivered
        });
        let (sender, receiver) = mpsc::channel();
        let reader = scope.spawn(move || {
            let mut reply = Vec::new();
            let read = output
                .take(MAX_BATCH_REPLY_BYTES as u64 + 2)
                .read_to_end(&mut reply);
            let _ = sender.send(read.map(|_| reply));
        });
        let result = (|| {
            let mut received = None;
            loop {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                if Instant::now() >= deadline {
                    return Err(Error::Deadline);
                }
                if let Ok(reply) = receiver.try_recv() {
                    let reply = reply.map_err(|_| Error::Worker)?;
                    if reply.len() > MAX_BATCH_REPLY_BYTES + 1 {
                        return Err(Error::Worker);
                    }
                    received = Some(reply);
                }
                if child.finished(5).map_err(|_| Error::Worker)? {
                    if !child.success().map_err(|_| Error::Worker)? {
                        return Err(Error::Worker);
                    }
                    return received.map_or_else(
                        || {
                            receiver
                                .recv()
                                .map_err(|_| Error::Worker)?
                                .map_err(|_| Error::Worker)
                        },
                        Ok,
                    );
                }
            }
        })();
        child.terminate();
        let delivered = writer.join().map_err(|_| Error::Worker)?;
        reader.join().map_err(|_| Error::Worker)?;
        protocol::worker_reply(result?, delivered)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::windows::tests::{PROBE_ARGS, UNREAD_PROBE_ARGS};

    #[test]
    fn supervisor_rejects_unsolicited_output_failed_exits_and_output_floods() {
        for request in [
            // The native probe runs under libtest, which writes a preamble.
            // Even an otherwise valid reply must reject unsolicited output.
            br#"{"action":"reply","status":0,"body":"[]","exit":0}"#.as_slice(),
            br#"{"action":"reply","status":0,"body":"[]","exit":1}"#,
            br#"{"action":"flood"}"#,
        ] {
            assert_eq!(
                supervise_arguments(
                    &std::env::current_exe().unwrap(),
                    PROBE_ARGS,
                    request.to_vec(),
                    &AtomicBool::new(false),
                    Duration::from_secs(5),
                    None
                ),
                Err(Error::Worker)
            );
        }
    }
    #[test]
    fn supervisor_deadline_reaps_a_worker_that_never_exits() {
        let start = Instant::now();
        let result = supervise_arguments(
            &std::env::current_exe().unwrap(),
            PROBE_ARGS,
            br#"{"action":"wait"}"#.to_vec(),
            &AtomicBool::new(false),
            Duration::from_secs(2),
            None,
        );
        assert_eq!(result, Err(Error::Deadline));
        assert!(start.elapsed() < Duration::from_secs(10));
    }
    #[test]
    fn supervisor_cancellation_unblocks_io_and_reaps() {
        let cancelled = AtomicBool::new(false);
        thread::scope(|scope| {
            let run = scope.spawn(|| {
                supervise_arguments(
                    &std::env::current_exe().unwrap(),
                    UNREAD_PROBE_ARGS,
                    vec![b'x'; 2 * 1024 * 1024],
                    &cancelled,
                    Duration::from_secs(10),
                    None,
                )
            });
            thread::sleep(Duration::from_millis(500));
            cancelled.store(true, Ordering::Relaxed);
            assert_eq!(run.join().unwrap(), Err(Error::Cancelled));
        });
    }

    #[test]
    fn premature_success_cannot_bypass_a_deadline_with_a_blocked_request_pipe() {
        assert_eq!(
            supervise_arguments(
                &std::env::current_exe().unwrap(),
                UNREAD_PROBE_ARGS,
                vec![b'x'; 2 * 1024 * 1024],
                &AtomicBool::new(false),
                Duration::from_secs(2),
                None
            ),
            Err(Error::Deadline)
        );
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::runtime::{self, engine, Input};
    fn input(source: &str, selections: Vec<String>) -> Input {
        Input {
            source: source.into(),
            selections,
            language: "markdown".into(),
            settings: [("prefix".into(), crate::options::Value::Text("→ ".into()))].into(),
        }
    }
    #[test]
    fn plugin_shared_engine_runs_unicode_settings_and_sixteen_selections() {
        let text = "λ".repeat(2048);
        let edits = engine::run(input("return function(text, language, settings) return settings.prefix .. text .. ':' .. language end", vec![text.clone(); 16]), None, &AtomicBool::new(false), false).unwrap();
        assert_eq!(edits, vec![format!("→ {text}:markdown"); 16]);
    }
    #[test]
    fn plugin_shared_engine_save_budget_uses_the_confined_worker() {
        let approval = tempfile::tempfile().unwrap();
        let result = runtime::run_save(
            input(
                "return function(text) return text:gsub(' +$', '') end",
                vec!["café\r\n東京  ".into()],
            ),
            approval,
        )
        .unwrap();
        assert_eq!(result, "café\r\n東京");
    }
}
