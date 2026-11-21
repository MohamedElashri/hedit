//! Bounded service lifecycle, independent of UI, IPC, and wall-clock sleeping.
use super::{sdk, Trigger};
const WINDOW_MS: u64 = 30_000;
const WINDOW_WORK_MS: u64 = 10_000;
const MIN_INTERVAL_MS: u64 = 100;
pub const IDLE_MS: u64 = 30_000;
#[derive(Clone, Copy, Debug)]
pub struct Pending {
    pub trigger: Trigger,
    pub generation: u64,
    pub due_ms: u64,
}
#[derive(Clone, Copy)]
struct Flight {
    generation: u64,
    started_ms: u64,
    paused_at: Option<u64>,
    excluded_ms: u64,
}
#[derive(Default)]
pub struct State {
    pub cache: sdk::Storage,
    pub quarantined: bool,
    pub stopped: bool,
    pending: Option<Pending>,
    flight: Option<Flight>,
    generation: u64,
    last_start: Option<u64>,
    last_activity: u64,
    window_start: u64,
    spent_ms: u64,
    failures: u8,
    backoff_until: u64,
}
impl State {
    pub fn queue(&mut self, trigger: Trigger, now: u64, debounce: u16) {
        if self.quarantined || self.stopped {
            return;
        }
        let Some(generation) = self.generation.checked_add(1) else {
            self.quarantined = true;
            self.pending = None;
            return;
        };
        self.generation = generation;
        self.last_activity = now;
        let delay = if matches!(
            trigger,
            Trigger::Manual | Trigger::Interaction | Trigger::Close
        ) {
            0
        } else {
            u64::from(debounce)
        };
        self.pending = Some(Pending {
            trigger,
            generation,
            due_ms: now.saturating_add(delay),
        });
    }
    pub fn defer(&mut self, now: u64, delay: u16) {
        if let Some(pending) = &mut self.pending {
            pending.due_ms = pending.due_ms.max(now.saturating_add(u64::from(delay)));
        }
    }
    pub fn superseded(&self) -> bool {
        self.flight.is_some_and(|f| f.generation != self.generation)
    }
    pub fn begin(&mut self, now: u64) -> Option<Pending> {
        if self.quarantined || self.stopped || self.flight.is_some() {
            return None;
        }
        if now.saturating_sub(self.window_start) >= WINDOW_MS {
            self.window_start = now;
            self.spent_ms = 0;
        }
        let pending = self.pending?;
        if now < pending.due_ms
            || now < self.backoff_until
            || self
                .last_start
                .is_some_and(|last| now.saturating_sub(last) < MIN_INTERVAL_MS)
            || self.spent_ms >= WINDOW_WORK_MS
        {
            return None;
        }
        self.pending = None;
        self.last_start = Some(now);
        self.flight = Some(Flight {
            generation: pending.generation,
            started_ms: now,
            paused_at: None,
            excluded_ms: 0,
        });
        Some(pending)
    }
    pub fn pause_work(&mut self, now: u64) {
        if let Some(flight) = &mut self.flight {
            if flight.paused_at.is_none() {
                flight.paused_at = Some(now);
            }
        }
    }
    pub fn resume_work(&mut self, now: u64) {
        if let Some(flight) = &mut self.flight {
            if let Some(paused) = flight.paused_at.take() {
                flight.excluded_ms = flight
                    .excluded_ms
                    .saturating_add(now.saturating_sub(paused));
            }
        }
    }
    fn charge(&mut self, now: u64) -> Option<Flight> {
        self.charge_work(now, None)
    }
    fn charge_work(&mut self, now: u64, work: Option<u64>) -> Option<Flight> {
        let flight = self.flight.take()?;
        self.spent_ms = self.spent_ms.saturating_add(work.unwrap_or_else(|| {
            flight
                .paused_at
                .unwrap_or(now)
                .saturating_sub(flight.started_ms)
                .saturating_sub(flight.excluded_ms)
        }));
        Some(flight)
    }
    pub fn finish(
        &mut self,
        generation: u64,
        now: u64,
        success: bool,
        cache: Option<sdk::Storage>,
    ) -> bool {
        self.finish_accounted(generation, now, None, success, cache)
    }
    /// The scoped host accounts aggregate work separately from human preview
    /// dwell and parked I/O, so elapsed UI time is not charged as computation.
    pub fn finish_with_work(
        &mut self,
        generation: u64,
        now: u64,
        work: u64,
        success: bool,
        cache: Option<sdk::Storage>,
    ) -> bool {
        self.finish_accounted(generation, now, Some(work), success, cache)
    }
    fn finish_accounted(
        &mut self,
        generation: u64,
        now: u64,
        work: Option<u64>,
        success: bool,
        cache: Option<sdk::Storage>,
    ) -> bool {
        if self
            .flight
            .is_none_or(|flight| flight.generation != generation)
        {
            return false;
        }
        let Some(flight) = self.charge_work(now, work) else {
            return false;
        };
        if flight.generation != generation || generation != self.generation {
            return false;
        }
        if success {
            self.failures = 0;
            if let Some(cache) = cache {
                self.cache = cache;
            }
        } else {
            self.failures = self.failures.saturating_add(1);
            self.backoff_until =
                now.saturating_add(500u64.saturating_mul(1u64 << self.failures.min(5)));
            if self.failures >= 3 {
                self.quarantined = true;
                self.pending = None;
                self.cache = Default::default();
            }
        }
        true
    }
    pub fn cancel(&mut self, now: u64) {
        self.charge(now);
    }
    pub fn stop(&mut self, now: u64) {
        self.cancel(now);
        self.stopped = true;
        self.pending = None;
        self.cache = Default::default();
    }
    pub fn resume(&mut self) {
        let generation = self.generation.checked_add(1);
        *self = Self::default();
        if let Some(generation) = generation {
            self.generation = generation;
        } else {
            self.quarantined = true;
        }
    }
    pub fn invalidate_request(&mut self, now: u64) {
        self.cancel(now);
        self.pending = None;
        if let Some(generation) = self.generation.checked_add(1) {
            self.generation = generation;
        } else {
            self.quarantined = true;
            self.cache = Default::default();
        }
    }
    pub fn change_scope(&mut self, now: u64) {
        self.invalidate_request(now);
        self.cache = Default::default();
        self.last_activity = now;
    }
    pub fn expire_idle(&mut self, now: u64) -> bool {
        if self.flight.is_none()
            && self.pending.is_none()
            && now.saturating_sub(self.last_activity) >= IDLE_MS
        {
            let changed = !self.cache.0.is_empty();
            self.cache = Default::default();
            changed
        } else {
            false
        }
    }
    pub fn next_due(&self, now: u64) -> Option<u64> {
        if self.stopped || self.quarantined || self.flight.is_some() {
            return None;
        }
        self.pending
            .map(|p| {
                p.due_ms
                    .max(self.backoff_until)
                    .max(
                        self.last_start
                            .map_or(0, |last| last.saturating_add(MIN_INTERVAL_MS)),
                    )
                    .max(if self.spent_ms >= WINDOW_WORK_MS {
                        self.window_start.saturating_add(WINDOW_MS)
                    } else {
                        0
                    })
                    .max(now)
            })
            .or_else(|| {
                (!self.cache.0.is_empty())
                    .then_some(self.last_activity.saturating_add(IDLE_MS).max(now))
            })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storms_coalesce_and_late_results_cannot_restore_scope_data() {
        let mut state = State::default();
        for time in 0..100_000 {
            state.queue(Trigger::Change, time, 150);
        }
        assert!(state.begin(100_000).is_none());
        let request = state.begin(100_200).unwrap();
        state.queue(Trigger::Selection, 100_201, 150);
        assert!(state.superseded());
        let secret = sdk::Storage(
            [(
                "secret".into(),
                crate::options::Value::Text("private fixture".into()),
            )]
            .into(),
        );
        assert!(!state.finish(request.generation, 100_250, true, Some(secret)));
        assert!(state.cache.0.is_empty());
        let latest = state.begin(100_351).unwrap();
        assert_eq!(latest.trigger, Trigger::Selection);
        state.change_scope(100_352);
        assert!(!state.finish(latest.generation, 100_400, true, None));
    }
    #[test]
    fn errors_quarantine_without_a_restart_loop_and_scope_changes_do_not_bypass_it() {
        let mut state = State::default();
        for n in 0..3 {
            let now = n * 10_000;
            state.queue(Trigger::Manual, now, 150);
            let request = state.begin(now).unwrap();
            assert!(state.finish(request.generation, now + 1, false, None));
        }
        assert!(state.quarantined);
        state.change_scope(40_000);
        state.queue(Trigger::Open, 40_000, 150);
        assert!(state.begin(50_000).is_none());
        assert!(state.next_due(50_000).is_none());
        state.resume();
        state.queue(Trigger::Manual, 50_000, 150);
        assert!(state.begin(50_000).is_some());
    }
    #[test]
    fn work_windows_idle_expiry_and_stop_are_bounded() {
        let mut state = State::default();
        for n in 0..20 {
            let now = n * 501;
            state.queue(Trigger::Manual, now, 150);
            let Some(request) = state.begin(now) else {
                break;
            };
            state.finish(request.generation, now + 500, true, None);
        }
        state.queue(Trigger::Manual, 11_000, 150);
        assert!(state.begin(11_000).is_none());
        assert_eq!(state.next_due(11_000), Some(30_000));
        assert!(state.begin(30_000).is_some());
        state.stop(30_001);
        assert!(state.next_due(40_000).is_none());
        assert!(state.begin(50_000).is_none());
        state.resume();
        state.cache = sdk::Storage([("count".into(), crate::options::Value::Integer(1))].into());
        assert!(state.expire_idle(30_000));
        assert!(state.cache.0.is_empty());
    }
    #[test]
    fn preview_dwell_is_not_work_even_when_cancelled_and_host_accounting_excludes_parked_time() {
        let mut state = State::default();
        state.queue(Trigger::Manual, 0, 0);
        let first = state.begin(0).unwrap();
        state.pause_work(30);
        state.resume_work(20_030);
        assert!(state.finish(first.generation, 20_060, true, None));
        assert_eq!(state.spent_ms, 60);
        state.queue(Trigger::Manual, 20_160, 0);
        state.begin(20_160).unwrap();
        state.pause_work(20_200);
        state.cancel(29_000);
        assert_eq!(state.spent_ms, 100);
        state.queue(Trigger::Manual, 29_100, 0);
        let next = state.begin(29_100).unwrap();
        assert!(state.finish_with_work(next.generation, 40_000, 12, true, None));
        assert_eq!(state.spent_ms, 112);
    }
    #[test]
    fn foreign_completion_cannot_consume_a_new_flight_or_restore_prior_cache() {
        let mut state = State::default();
        state.queue(Trigger::Manual, 0, 150);
        let old = state.begin(0).unwrap();
        state.stop(1);
        state.resume();
        state.queue(Trigger::Manual, 101, 150);
        let current = state.begin(101).unwrap();
        assert!(!state.finish(old.generation, 102, true, None));
        assert!(state.finish(current.generation, 103, true, None));
        state.cache = sdk::Storage([("value".into(), crate::options::Value::Integer(1))].into());
        state.invalidate_request(104);
        assert_eq!(state.cache.0.len(), 1);
        state.change_scope(105);
        assert!(state.cache.0.is_empty());
        state.generation = u64::MAX;
        state.invalidate_request(106);
        assert!(state.quarantined);
        assert!(state.next_due(106).is_none());
    }
}
