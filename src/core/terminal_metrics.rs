//! Bounded aggregate graphics and presentation measurements on the calling UI thread.
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Instant;

#[cfg(feature = "terminal")]
use crate::widgets::{GraphicsDamage, TerminalDamage, TerminalMutation};

/// Cumulative counters for parsers and host backends on the calling thread.
///
/// Native operations and command bytes count committed scene transitions. Failed attempts are
/// counted separately; inspect an outgoing-wire trace when measuring partially written failures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct TerminalGraphicsMetrics {
    /// Parser calls without completed mutations.
    pub mutations_none: u64,
    /// Parser calls with metadata or responses only.
    pub mutations_metadata: u64,
    /// Graphics-only parser mutations.
    pub mutations_graphics: u64,
    /// Parser mutations affecting cells or the cursor.
    pub mutations_rows: u64,
    /// Parser mutations requiring full terminal damage.
    pub mutations_full: u64,
    /// Logical sources first uploaded by a successful scene transition.
    pub sources_created: u64,
    /// Source uploads committed after successful host output.
    pub sources_uploaded: u64,
    /// Desired sources reused without upload.
    pub sources_unchanged: u64,
    /// Existing logical sources uploaded at a newer revision.
    pub sources_updated: u64,
    /// Host sources deleted by committed scene transitions.
    pub sources_retired: u64,
    /// Rectangles considered by native reconciliation.
    pub placements_desired: u64,
    /// Host placements created by committed scene transitions.
    pub placements_created: u64,
    /// Desired placements requiring no host command.
    pub placements_unchanged: u64,
    /// Existing placements updated or refreshed after a source upload.
    pub placements_updated: u64,
    /// Host placements deleted by committed scene transitions.
    pub placements_deleted: u64,
    /// Visible scanline regions considered for rectangle merging.
    pub regions_before_merge: u64,
    /// Rectangles produced by bounded vertical merging.
    pub regions_after_merge: u64,
    /// Successful direct graphics presentations.
    pub graphics_direct: u64,
    /// Graphics requests widened to terminal cell painting.
    pub graphics_widened_terminal: u64,
    /// Graphics requests widened to ordinary tree painting.
    pub graphics_widened_normal: u64,
    /// Upload command bytes in committed scene transitions.
    pub upload_bytes: u64,
    /// Placement command bytes in committed scene transitions.
    pub placement_bytes: u64,
    /// Deletion command bytes in committed scene transitions.
    pub delete_bytes: u64,
    /// Host frame attempts, including direct graphics attempts.
    pub frames_requested: u64,
    /// Host frame attempts that paint cells.
    pub frames_composed: u64,
    /// Complete successful, undiscarded host frames.
    pub frames_written: u64,
    /// Abandoned or discarded host frame attempts.
    pub frames_failed: u64,
    /// Total duration of normal frames, excluding backend flush time.
    pub compose_us: u64,
    /// Total backend flush duration, including failed attempts.
    pub host_write_us: u64,
    /// Median successful frame interval over the newest 1024 intervals.
    pub frame_interval_p50_us: u64,
    /// 95th percentile successful frame interval.
    pub frame_interval_p95_us: u64,
    /// 99th percentile successful frame interval.
    pub frame_interval_p99_us: u64,
    /// Largest successful frame interval in the bounded window.
    pub frame_interval_max_us: u64,
}

#[derive(Default)]
struct Measurements {
    counters: TerminalGraphicsMetrics,
    intervals: VecDeque<u64>,
    previous_frame: Option<Instant>,
}

thread_local! {
    static METRICS: RefCell<Measurements> = RefCell::default();
}

/// Read cumulative graphics counters without enabling per-frame logs.
///
/// Each thread has independent aggregates. PTY reader threads do not paint; applications normally
/// call this on their UI thread. Frame interval percentiles retain at most 1024 successful writes.
pub fn terminal_graphics_metrics() -> TerminalGraphicsMetrics {
    METRICS.with(|metrics| {
        let metrics = metrics.borrow();
        let mut result = metrics.counters;
        let mut intervals: Vec<_> = metrics.intervals.iter().copied().collect();
        intervals.sort_unstable();
        let percentile = |percent: usize| {
            intervals
                .get((intervals.len() * percent).div_ceil(100).saturating_sub(1))
                .copied()
                .unwrap_or(0)
        };
        result.frame_interval_p50_us = percentile(50);
        result.frame_interval_p95_us = percentile(95);
        result.frame_interval_p99_us = percentile(99);
        result.frame_interval_max_us = intervals.last().copied().unwrap_or(0);
        result
    })
}

pub(crate) fn update(change: impl FnOnce(&mut TerminalGraphicsMetrics)) {
    METRICS.with(|metrics| change(&mut metrics.borrow_mut().counters));
}

#[cfg(feature = "terminal")]
pub(crate) fn mutation(mutation: &TerminalMutation) {
    update(|metrics| {
        let count = match &mutation.cells {
            TerminalDamage::Full => &mut metrics.mutations_full,
            TerminalDamage::Rows(_) => &mut metrics.mutations_rows,
            TerminalDamage::None if mutation.cursor_changed => &mut metrics.mutations_rows,
            TerminalDamage::None if mutation.graphics != GraphicsDamage::None => {
                &mut metrics.mutations_graphics
            }
            TerminalDamage::None if mutation.is_empty() => &mut metrics.mutations_none,
            TerminalDamage::None => &mut metrics.mutations_metadata,
        };
        *count += 1;
    });
}

pub(crate) fn presented(now: Instant) {
    METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        metrics.counters.frames_written += 1;
        if let Some(previous) = metrics.previous_frame.replace(now) {
            metrics.intervals.push_back(
                now.saturating_duration_since(previous)
                    .as_micros()
                    .min(u128::from(u64::MAX)) as u64,
            );
            if metrics.intervals.len() > 1024 {
                metrics.intervals.pop_front();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn frame_interval_window_is_bounded_and_reports_successful_writes() {
        METRICS.with(|metrics| *metrics.borrow_mut() = Measurements::default());
        let start = Instant::now();
        for frame in 0..1100 {
            presented(start + Duration::from_micros(frame * 1000));
        }
        let public: crate::TerminalGraphicsMetrics = crate::terminal_graphics_metrics();
        assert_eq!(public.frames_written, 1100);
        assert_eq!(
            (
                public.frame_interval_p50_us,
                public.frame_interval_p95_us,
                public.frame_interval_p99_us,
                public.frame_interval_max_us
            ),
            (1000, 1000, 1000, 1000)
        );
        METRICS.with(|metrics| assert_eq!(metrics.borrow().intervals.len(), 1024));
        update(|metrics| metrics.frames_failed += 1);
        assert_eq!(terminal_graphics_metrics().frames_written, 1100);
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn metadata_and_incomplete_input_do_not_count_as_visual_mutations() {
        METRICS.with(|metrics| *metrics.borrow_mut() = Measurements::default());
        mutation(&TerminalMutation::default());
        mutation(&TerminalMutation {
            responses: true,
            ..Default::default()
        });
        mutation(&TerminalMutation {
            graphics: GraphicsDamage::Sources,
            ..Default::default()
        });
        mutation(&TerminalMutation {
            cursor_changed: true,
            ..Default::default()
        });
        mutation(&TerminalMutation {
            cells: TerminalDamage::Full,
            ..Default::default()
        });
        let metrics = terminal_graphics_metrics();
        assert_eq!(
            (
                metrics.mutations_none,
                metrics.mutations_metadata,
                metrics.mutations_graphics,
                metrics.mutations_rows,
                metrics.mutations_full
            ),
            (1, 1, 1, 1, 1)
        );
    }
}
