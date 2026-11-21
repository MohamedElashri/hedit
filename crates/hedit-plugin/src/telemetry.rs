//! Fixed numeric phase measurements. No payloads, identifiers, or file paths.
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum Phase {
    Authorization,
    Startup,
    Delivery,
    Worker,
    Transport,
    Decode,
    Proposal,
    Commit,
    Prompt,
    Preview,
    Encode,
}
const PHASES: [Phase; 11] = [
    Phase::Authorization,
    Phase::Startup,
    Phase::Delivery,
    Phase::Worker,
    Phase::Transport,
    Phase::Decode,
    Phase::Proposal,
    Phase::Commit,
    Phase::Prompt,
    Phase::Preview,
    Phase::Encode,
];
#[derive(Default)]
struct Counter {
    micros: AtomicU64,
    samples: AtomicU64,
}
#[derive(Clone, Default)]
pub struct Recorder(Arc<[Counter; 11]>);
#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    pub phase: Phase,
    pub total_us: u64,
    pub count: u64,
}
impl Recorder {
    pub fn record(&self, phase: Phase, duration: Duration) {
        let counter = &self.0[phase as usize];
        let micros = duration.as_micros().min(u128::from(u64::MAX)) as u64;
        let mut previous = counter.micros.load(Ordering::Relaxed);
        loop {
            match counter.micros.compare_exchange_weak(
                previous,
                previous.saturating_add(micros),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => previous = actual,
            }
        }
        counter.samples.fetch_add(1, Ordering::Release);
    }
    pub fn measure<T>(&self, phase: Phase, call: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let result = call();
        self.record(phase, start.elapsed());
        result
    }
    /// A nonblocking numeric snapshot; concurrent phases may finish afterward.
    pub fn snapshot(&self) -> Vec<Sample> {
        PHASES
            .iter()
            .filter_map(|&phase| {
                let counter = &self.0[phase as usize];
                let count = counter.samples.load(Ordering::Acquire);
                (count > 0).then(|| Sample {
                    phase,
                    count,
                    total_us: counter.micros.load(Ordering::Relaxed),
                })
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_counters_preserve_parallel_samples_without_payload_data() {
        let recorder = Recorder::default();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let recorder = recorder.clone();
                scope.spawn(move || {
                    for _ in 0..1000 {
                        recorder.record(Phase::Commit, Duration::from_micros(3));
                    }
                });
            }
        });
        let samples = recorder.snapshot();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].count, 4000);
        assert_eq!(samples[0].total_us, 12000);
        assert_eq!(serde_json::to_value(samples).unwrap()[0]["phase"], "commit");
    }
}
