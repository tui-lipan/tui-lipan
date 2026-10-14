//! Per-reader credits, independent of the UI thread and other PTYs.
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub(crate) const PTY_READ_BUFFER_SIZE: usize = 8192;
/// Most bytes one credit may drain.
pub(crate) const MAXIMUM_CREDIT: usize = 64 * 1024;
const MAXIMUM_HOLD: Duration = Duration::from_millis(250);

/// What the reader may consume before it next consults the gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReadBudget {
    pub(crate) bytes: usize,
    /// Keep reading while the PTY has bytes immediately pending, up to `bytes` in total.
    pub(crate) drain: bool,
}

impl ReadBudget {
    const OPEN: Self = Self {
        bytes: PTY_READ_BUFFER_SIZE,
        drain: false,
    };

    fn credit(bytes: usize) -> Self {
        Self { bytes, drain: true }
    }
}

/// Aggregate reader backpressure, without per-read logging.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalReadGateStats {
    /// Number of times a reader actually waited for credit.
    pub holds: u64,
    /// Completed holds, plus the elapsed duration of a current hold.
    pub total_hold_time: Duration,
    /// Reads admitted by the bounded safety timeout.
    pub forced_releases: u64,
    /// Whether this reader is currently waiting for credit.
    pub held: bool,
}

/// A per-PTY read gate. Normal terminal output starts unrestricted.
///
/// Credited mode admits one drain per granted credit: the reader keeps reading while the PTY has
/// bytes immediately pending, up to the chosen byte budget, and delivers them as one output event.
/// Holding a producer leaves its output queued in the kernel, oldest first, so a release has to
/// take that whole backlog for a consumer to see current state instead of the oldest queued frame.
/// At most one previously admitted drain is in flight. Exhausted credit holds this reader only. A safety timeout always admits another bounded read; closing a child or explicitly
/// resuming the gate releases the reader so its final bytes can drain before the exit event.
#[derive(Clone, Default)]
pub struct TerminalReadGate(Arc<Gate>);

#[derive(Default)]
struct Gate {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    policy: Policy,
    finished: bool,
    held_since: Option<Instant>,
    stats: TerminalReadGateStats,
}

#[derive(Default)]
enum Policy {
    #[default]
    Open,
    Credit {
        available: bool,
        bytes: usize,
        maximum_hold: Duration,
    },
}

impl State {
    fn finish_hold(&mut self, now: Instant) {
        if let Some(start) = self.held_since.take() {
            self.stats.total_hold_time += now.saturating_duration_since(start);
        }
    }

    fn poll(&mut self, now: Instant) -> Option<ReadBudget> {
        let Policy::Credit {
            available,
            bytes,
            maximum_hold,
        } = &mut self.policy
        else {
            self.finish_hold(now);
            return Some(ReadBudget::OPEN);
        };
        if *available {
            *available = false;
            let bytes = *bytes;
            self.finish_hold(now);
            return Some(ReadBudget::credit(bytes));
        }
        let start = *self.held_since.get_or_insert_with(|| {
            self.stats.holds += 1;
            now
        });
        if now.saturating_duration_since(start) >= *maximum_hold {
            let bytes = *bytes;
            self.stats.forced_releases += 1;
            self.finish_hold(now);
            return Some(ReadBudget::credit(bytes));
        }
        None
    }

    fn remaining_hold(&self, now: Instant) -> Duration {
        match (&self.policy, self.held_since) {
            (Policy::Credit { maximum_hold, .. }, Some(start)) => {
                maximum_hold.saturating_sub(now.saturating_duration_since(start))
            }
            _ => Duration::ZERO,
        }
    }
}

impl TerminalReadGate {
    /// Set a bounded drain budget and grant or withhold one credit. Repeated withheld-credit
    /// updates do not extend a hold already in progress. Budgets clamp to 1 byte–64 KiB and safety
    /// holds clamp to 1–250 ms. A budget bounds how much queued output one release may parse;
    /// one smaller than the backlog delivers its oldest part and leaves the rest queued.
    pub fn set_credit(&self, bytes: usize, available: bool, maximum_hold: Duration) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.finished {
            return;
        }
        state.policy = Policy::Credit {
            available,
            bytes: bytes.clamp(1, MAXIMUM_CREDIT),
            maximum_hold: maximum_hold.clamp(Duration::from_millis(1), MAXIMUM_HOLD),
        };
        self.0.changed.notify_all();
    }

    /// Release this reader immediately and restore unrestricted reads.
    pub fn resume(&self) {
        self.set_open(false);
    }

    pub(crate) fn finish(&self) {
        self.set_open(true);
    }

    fn set_open(&self, finished: bool) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.finish_hold(Instant::now());
        state.finished |= finished;
        state.policy = Policy::Open;
        self.0.changed.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn test_hold_limit(&self, maximum: Duration) {
        if let Policy::Credit { maximum_hold, .. } = &mut self.0.state.lock().unwrap().policy {
            *maximum_hold = maximum;
        }
    }

    #[cfg(test)]
    pub(crate) fn wait_for_hold(&self) -> bool {
        let state = self.0.state.lock().unwrap();
        let (_state, timeout) = self
            .0
            .changed
            .wait_timeout_while(state, Duration::from_secs(1), |state| {
                state.held_since.is_none()
            })
            .unwrap();
        !timeout.timed_out()
    }

    /// Current aggregate hold metrics.
    pub fn stats(&self) -> TerminalReadGateStats {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stats = state.stats;
        stats.held = state.held_since.is_some();
        if let Some(start) = state.held_since {
            stats.total_hold_time += start.elapsed();
        }
        stats
    }

    pub(crate) fn read_budget(&self) -> ReadBudget {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            let now = Instant::now();
            if let Some(bytes) = state.poll(now) {
                return bytes;
            }
            self.0.changed.notify_all();
            let timeout = state.remaining_hold(now);
            let (next, _) = self
                .0
                .changed
                .wait_timeout(state, timeout)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_credit_admits_one_bounded_read_and_timeout_releases_safely() {
        let now = Instant::now();
        let mut state = State {
            policy: Policy::Credit {
                available: true,
                bytes: 128,
                maximum_hold: Duration::from_millis(100),
            },
            ..Default::default()
        };
        assert_eq!(state.poll(now), Some(ReadBudget::credit(128)));
        assert_eq!(state.poll(now), None);
        assert_eq!(state.poll(now + Duration::from_millis(99)), None);
        assert_eq!(
            state.poll(now + Duration::from_millis(100)),
            Some(ReadBudget::credit(128))
        );
        assert_eq!(state.stats.holds, 1);
        assert_eq!(state.stats.forced_releases, 1);
        assert_eq!(state.stats.total_hold_time, Duration::from_millis(100));
    }

    #[test]
    fn changing_a_withheld_credit_cannot_extend_the_current_hold() {
        let now = Instant::now();
        let mut state = State {
            policy: Policy::Credit {
                available: false,
                bytes: 64,
                maximum_hold: Duration::from_millis(100),
            },
            ..Default::default()
        };
        assert_eq!(state.poll(now), None);
        state.policy = Policy::Credit {
            available: false,
            bytes: 256,
            maximum_hold: Duration::from_millis(100),
        };
        assert_eq!(
            state.poll(now + Duration::from_millis(100)),
            Some(ReadBudget::credit(256))
        );
        assert_eq!(state.stats.forced_releases, 1);
    }

    #[test]
    fn resuming_one_gate_releases_its_reader_without_touching_another() {
        let gate = TerminalReadGate::default();
        let other = TerminalReadGate::default();
        gate.set_credit(128, false, MAXIMUM_HOLD);
        gate.test_hold_limit(Duration::from_secs(30));
        other.set_credit(64, true, MAXIMUM_HOLD);
        let reader_gate = gate.clone();
        let reader = std::thread::spawn(move || reader_gate.read_budget());
        let state = gate.0.state.lock().unwrap();
        let (state, timeout) = gate
            .0
            .changed
            .wait_timeout_while(state, Duration::from_secs(1), |state| {
                state.held_since.is_none()
            })
            .unwrap();
        assert!(!timeout.timed_out());
        drop(state);
        assert_eq!(other.read_budget(), ReadBudget::credit(64));
        gate.resume();
        assert_eq!(reader.join().unwrap(), ReadBudget::OPEN);
        assert_eq!(gate.stats().holds, 1);
        assert!(!gate.stats().held);
        assert_eq!(gate.stats().forced_releases, 0);
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;
    #[test]
    fn exit_permanently_disables_credit_holds_for_final_output() {
        let gate = TerminalReadGate::default();
        gate.set_credit(64, false, MAXIMUM_HOLD);
        gate.finish();
        gate.set_credit(64, false, MAXIMUM_HOLD);
        assert_eq!(gate.read_budget(), ReadBudget::OPEN);
        assert_eq!(gate.stats().holds, 0);
    }
}
