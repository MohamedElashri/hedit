//! Host-owned duplex transport; no framing, method or document authority.
use super::{Admission, Launch};
use std::{
    io,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

#[cfg(any(
    target_os = "macos",
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
use super::unix::Stream as Native;
#[cfg(windows)]
use super::windows::Stream as Native;

static SERVICES: AtomicUsize = AtomicUsize::new(0);
const BYTES: usize = 16 * 1024 * 1024;
const LIFETIME: Duration = Duration::from_secs(120);
pub const CHUNK: usize = 4096;

#[derive(Default, Debug)]
pub struct Progress {
    pub written: usize,
    pub read: usize,
}
impl Progress {
    pub fn idle(&self) -> bool {
        self.written == 0 && self.read == 0
    }
}
struct Lease {
    _global: Admission,
}
impl Lease {
    fn acquire() -> io::Result<Self> {
        let admission = Admission::acquire()?;
        let mut active = SERVICES.load(Ordering::Acquire);
        loop {
            if active >= 3 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Native service capacity is exhausted.",
                ));
            }
            match SERVICES.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(current) => active = current,
            }
        }
        Ok(Self { _global: admission })
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        SERVICES.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The host worker must continuously poll, including while idle, and drop on
/// policy/scope cancellation. Drop kills and reaps before the admission lease
/// is released. Keep the private run bundle alive until after this owner drops.
/// The native two-second cumulative CPU ceiling is deliberately unchanged.
pub struct Stream {
    #[cfg(any(
        windows,
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    native: Option<Native>,
    expires: Instant,
    input: usize,
    output: usize,
    _lease: Lease,
}
impl Stream {
    pub fn open(launch: &Launch, cancel: &AtomicBool, deadline: Instant) -> io::Result<Self> {
        let lease = Lease::acquire()?;
        launch.validate()?;
        check(cancel, deadline)?;
        #[cfg(any(
            windows,
            target_os = "macos",
            all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        ))]
        {
            let deadline = deadline.min(Instant::now() + Duration::from_secs(10));
            let native = Native::open(launch, cancel, deadline)?;
            Ok(Self {
                native: Some(native),
                expires: Instant::now() + LIFETIME,
                input: 0,
                output: 0,
                _lease: lease,
            })
        }
        #[cfg(not(any(
            windows,
            target_os = "macos",
            all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        )))]
        {
            let _ = lease;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Tool confinement is unavailable.",
            ))
        }
    }
    /// One nonblocking bounded transfer. Advance input only by `written` and
    /// consume only `output[..read]`; drain framing before another transfer.
    /// Any error permanently closes and reaps this connection.
    pub fn poll(
        &mut self,
        input: &[u8],
        output: &mut [u8; CHUNK],
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> io::Result<Progress> {
        let result = (|| {
            check(cancel, deadline.min(self.expires))?;
            #[cfg(any(
                windows,
                target_os = "macos",
                all(
                    target_os = "linux",
                    any(target_arch = "x86_64", target_arch = "aarch64")
                )
            ))]
            {
                let native = self
                    .native
                    .as_mut()
                    .ok_or_else(|| io::Error::other("Native stream is closed."))?;
                let progress = native.poll(&input[..input.len().min(CHUNK)], output)?;
                self.input = self.input.checked_add(progress.written).ok_or_else(limit)?;
                self.output = self.output.checked_add(progress.read).ok_or_else(limit)?;
                if self.input > BYTES || self.output > BYTES {
                    return Err(limit());
                }
                Ok(progress)
            }
            #[cfg(not(any(
                windows,
                target_os = "macos",
                all(
                    target_os = "linux",
                    any(target_arch = "x86_64", target_arch = "aarch64")
                )
            )))]
            {
                let _ = (input, output);
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Tool confinement is unavailable.",
                ))
            }
        })();
        if result.is_err() {
            #[cfg(any(
                windows,
                target_os = "macos",
                all(
                    target_os = "linux",
                    any(target_arch = "x86_64", target_arch = "aarch64")
                )
            ))]
            self.native.take();
        }
        result
    }
    /// Wait at most 50 ms for pipe readiness. The bounded wait also schedules
    /// cancellation/resource checks without a busy loop while the server idles.
    pub fn wait(&self, input_pending: bool) -> io::Result<()> {
        #[cfg(any(
            windows,
            target_os = "macos",
            all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        ))]
        return self
            .native
            .as_ref()
            .ok_or_else(|| io::Error::other("Native stream is closed."))?
            .wait(input_pending);
        #[cfg(not(any(
            windows,
            target_os = "macos",
            all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        )))]
        {
            let _ = input_pending;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Tool confinement is unavailable.",
            ))
        }
    }
}
pub(super) fn check(cancel: &AtomicBool, deadline: Instant) -> io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Native service cancelled.",
        ))
    } else if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Native service deadline exceeded.",
        ))
    } else {
        Ok(())
    }
}
fn limit() -> io::Error {
    io::Error::other("Native service transport budget exhausted.")
}
