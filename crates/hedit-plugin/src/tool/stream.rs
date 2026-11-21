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
const BYTES: usize = 8 * 1024 * 1024;
const LIFETIME: Duration = Duration::from_secs(30 * 60);
const WINDOW: Duration = Duration::from_secs(5);
/// Three five-second buckets conservatively bound every ten-second interval.
/// Charges survive the first boundary; no callback can reset accounting.
pub(super) struct Rate {
    epoch: Instant,
    buckets: [u64; 3],
    maximum: u64,
}
impl Rate {
    pub(super) fn new(now: Instant, maximum: u64) -> Self {
        Self {
            epoch: now,
            buckets: [0; 3],
            maximum,
        }
    }
    pub(super) fn charge(&mut self, now: Instant, amount: u64) -> io::Result<()> {
        let steps = now.saturating_duration_since(self.epoch).as_secs() / WINDOW.as_secs();
        if steps >= 3 {
            self.buckets = [0; 3];
            self.epoch = now;
        } else if steps != 0 {
            self.buckets.rotate_left(steps as usize);
            self.buckets[3 - steps as usize..].fill(0);
            self.epoch += WINDOW * steps as u32;
        }
        self.buckets[2] = self.buckets[2].checked_add(amount).ok_or_else(limit)?;
        let total = self
            .buckets
            .iter()
            .try_fold(0u64, |sum, n| sum.checked_add(*n))
            .ok_or_else(limit)?;
        if total > self.maximum {
            return Err(limit());
        }
        Ok(())
    }
}
pub(super) struct Cpu {
    rate: Rate,
    sampled: Duration,
}
impl Cpu {
    fn new(now: Instant) -> Self {
        Self {
            rate: Rate::new(now, 2_000_000_000),
            sampled: Duration::ZERO,
        }
    }
    fn charge(&mut self, now: Instant, total: Duration) -> io::Result<()> {
        let delta = total.checked_sub(self.sampled).ok_or_else(limit)?;
        if total >= Duration::from_secs(super::Role::Service.cpu_seconds()) {
            return Err(limit());
        }
        self.rate
            .charge(now, u64::try_from(delta.as_nanos()).map_err(|_| limit())?)?;
        self.sampled = total;
        Ok(())
    }
}
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
/// Services have independent cumulative and rolling CPU/traffic ceilings.
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
    input: Rate,
    output: Rate,
    cpu: Cpu,
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
                input: Rate::new(Instant::now(), BYTES as u64),
                output: Rate::new(Instant::now(), BYTES as u64),
                cpu: Cpu::new(Instant::now()),
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
                let now = Instant::now();
                self.cpu.charge(now, native.cpu_time())?;
                self.input.charge(now, progress.written as u64)?;
                self.output.charge(now, progress.read as u64)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rolling_rate_does_not_reset_at_first_boundary() {
        let now = Instant::now();
        let mut rate = Rate::new(now, 8);
        rate.charge(now, 8).unwrap();
        assert!(rate.charge(now + WINDOW, 1).is_err());
    }
    #[test]
    fn late_bucket_charge_survives_two_boundaries() {
        let now = Instant::now();
        let mut rate = Rate::new(now, 8);
        rate.charge(now + WINDOW - Duration::from_millis(1), 8)
            .unwrap();
        assert!(rate.charge(now + WINDOW * 2, 1).is_err());
    }
    #[test]
    fn rolling_rate_renews_after_three_buckets() {
        let now = Instant::now();
        let mut rate = Rate::new(now, 8);
        rate.charge(now, 8).unwrap();
        rate.charge(now + WINDOW * 3, 8).unwrap();
        assert!(rate.charge(now + WINDOW * 3, 1).is_err());
    }
    #[test]
    fn skipped_windows_and_integer_overflow_cannot_disable_limit() {
        let now = Instant::now();
        let mut rate = Rate::new(now, u64::MAX);
        rate.charge(now, u64::MAX).unwrap();
        assert!(rate.charge(now, 1).is_err());
        rate.charge(now + WINDOW * 100, u64::MAX).unwrap();
        assert!(rate.charge(now + WINDOW * 100, 1).is_err());
    }
    #[test]
    fn cpu_allows_use_beyond_filter_lifetime_but_rejects_bursts_and_rollback() {
        let now = Instant::now();
        let mut cpu = Cpu::new(now);
        cpu.charge(now, Duration::from_secs(1)).unwrap();
        cpu.charge(now + WINDOW * 3, Duration::from_secs(3))
            .unwrap();
        assert!(cpu
            .charge(now + WINDOW * 3, Duration::from_millis(3001))
            .is_err());
        assert!(cpu
            .charge(now + WINDOW * 3, Duration::from_secs(2))
            .is_err());
        let mut cpu = Cpu::new(now);
        assert!(cpu.charge(now, Duration::from_secs(60)).is_err());
    }
}
