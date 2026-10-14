//! One phase-preserving deadline for continuous presentation work.
use std::time::Duration;
use web_time::Instant;

pub(super) struct FrameClock {
    interval: Duration,
    next: Option<Instant>,
}

impl FrameClock {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: None,
        }
    }

    pub(super) fn due(&self, continuous: bool, now: Instant) -> Option<Instant> {
        continuous
            .then_some(self.next)
            .flatten()
            .filter(|due| now < *due)
    }

    /// Advance from the previous slot, skipping missed slots without changing phase.
    /// An immediate frame consumes a slot only once its deadline has arrived.
    pub(super) fn presented(&mut self, now: Instant) {
        let Some(next) = self.next else {
            self.next = Some(now + self.interval);
            return;
        };
        if now < next {
            return;
        }
        let missed = now.duration_since(next).as_nanos() / self.interval.as_nanos();
        let slots = u32::try_from(missed + 1).unwrap_or(u32::MAX);
        self.next = Some(next + self.interval.saturating_mul(slots));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_producers_keep_every_slot_at_all_supported_rates() {
        for hz in [60, 120, 144, 240] {
            let interval = Duration::from_secs_f64(1.0 / f64::from(hz));
            let origin = Instant::now();
            let mut clock = FrameClock::new(interval);
            clock.presented(origin);
            for frame in 1..=1000 {
                let due = origin + interval * frame;
                assert_eq!(clock.due(true, due - Duration::from_micros(10)), Some(due));
                assert_eq!(clock.due(true, due), None);
                // A small constant host write cost must not accumulate into the cadence.
                clock.presented(due + Duration::from_micros(100));
                assert_eq!(clock.next, Some(origin + interval * (frame + 1)));
            }
        }
    }

    #[test]
    fn late_and_immediate_frames_preserve_phase() {
        let interval = Duration::from_millis(10);
        let origin = Instant::now();
        let mut clock = FrameClock::new(interval);
        clock.presented(origin);
        assert_eq!(clock.due(false, origin + Duration::from_millis(2)), None);
        clock.presented(origin + Duration::from_millis(2));
        assert_eq!(clock.next, Some(origin + interval));
        clock.presented(origin + Duration::from_millis(37));
        assert_eq!(clock.next, Some(origin + Duration::from_millis(40)));
    }
}
