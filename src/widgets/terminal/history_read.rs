//! Bounded, non-blocking history reconstruction for an idle alternate-screen application.
//!
//! The owner feeds settled screen observations and executes returned wheel actions. It must hold
//! an exclusive input lease, prohibit raw attachments, and interrupt on input, resize, lifecycle
//! change, or unrelated output. No result is released until the original bottom viewport returns.
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::TerminalViewport;

/// One rendered row. A soft wrap joins this row directly to the next one in the result.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HistoryReadRow {
    /// Rendered cells, with terminal padding removed.
    pub text: Arc<str>,
    /// This row continues onto the next grid row.
    pub soft_wrapped: bool,
}

/// A complete observation at the application's real terminal dimensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryReadFrame {
    /// Real terminal dimensions. They must remain constant throughout a read.
    pub viewport: TerminalViewport,
    /// Exactly `viewport.rows` rendered rows.
    pub rows: Vec<HistoryReadRow>,
    /// A synchronized terminal update is open. Such observations cannot settle.
    pub synchronized: bool,
}

/// Why no history can be returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HistoryReadError {
    /// Input, resize, application activity, or another owner invalidated the read.
    #[error("history read interrupted")]
    Interrupted,
    /// The initial down probe moved the transcript, so it was manually scrolled.
    #[error("application transcript is not at its bottom")]
    NotAtBottom,
    /// Scrolling did not reveal a page with a unique, strong overlap.
    #[error("history pages do not have a unique strong overlap")]
    AmbiguousOverlap,
    /// No scrollable history was observed.
    #[error("scrollable application history is unavailable")]
    Unavailable,
    /// A traversal time, page, or byte bound was reached.
    #[error("history traversal limit reached")]
    Limit,
    /// The original viewport could not be proven restored. No text is returned.
    #[error("original viewport restoration could not be proven")]
    RestoreUnproven,
}

/// One step for the owner's event loop. Each scroll is executed exactly once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryReadStep {
    /// Continue pumping the application and call `observe` again.
    Wait,
    /// Send one upward wheel tick in the application's active mouse protocol.
    ScrollUp,
    /// Send one downward wheel tick in the application's active mouse protocol.
    ScrollDown,
    /// The read ended. Only `Ok` contains text, after restoration was proven.
    Finished(Result<String, HistoryReadError>),
}

#[derive(Clone, Copy)]
enum Phase {
    Settle,
    Probe,
    UndoProbe,
    Harvest,
    Restore,
    Cancelled,
    Finished,
}

/// Pure state machine for alternate-screen history. It never sends input or blocks the caller.
///
/// The initial frame must belong to a recognized, idle application with mouse reporting enabled,
/// no terminal scrollback offset, and no direct attachment. The owner enforces these preconditions
/// and holds its input lease through `Finished`, including failed reads and restoration.
pub struct AltScreenHistoryReader {
    original: HistoryReadFrame,
    observed: HistoryReadFrame,
    page: Vec<HistoryReadRow>,
    content: VecDeque<HistoryReadRow>,
    phase: Phase,
    started: Instant,
    action_at: Instant,
    changed_at: Instant,
    restore_at: Option<Instant>,
    /// The last action sent was an upward wheel tick whose response may still be in flight.
    pending_up: bool,
    changed: bool,
    pages: usize,
    crop: Option<(usize, usize)>,
    error: Option<HistoryReadError>,
}

const QUIET: Duration = Duration::from_millis(200);
const RESPONSE: Duration = Duration::from_millis(600);
const TRAVERSAL: Duration = Duration::from_secs(10);
const RESTORATION: Duration = Duration::from_secs(3);
const MAX_PAGES: usize = 128;
const MAX_BYTES: usize = 1024 * 1024;
const MIN_OVERLAP: usize = 3;

impl AltScreenHistoryReader {
    /// Start settling the given bottom viewport. A malformed or tiny frame is rejected.
    pub fn new(frame: HistoryReadFrame, now: Instant) -> Result<Self, HistoryReadError> {
        if frame.rows.len() != usize::from(frame.viewport.rows)
            || frame.viewport.cols == 0
            || frame.rows.len() <= MIN_OVERLAP
            || frame
                .rows
                .iter()
                .map(|row| row.text.len() + 1)
                .sum::<usize>()
                > MAX_BYTES
        {
            return Err(HistoryReadError::Unavailable);
        }
        Ok(Self {
            page: frame.rows.clone(),
            content: frame.rows.iter().cloned().collect(),
            original: frame.clone(),
            observed: frame,
            phase: Phase::Settle,
            started: now,
            action_at: now,
            changed_at: now,
            restore_at: None,
            pending_up: false,
            changed: false,
            pages: 0,
            crop: None,
            error: None,
        })
    }

    /// Interrupt without dropping the input lease. Keep observing until restoration finishes.
    pub fn interrupt(&mut self, now: Instant) {
        if matches!(self.phase, Phase::Finished) {
            return;
        }
        if matches!(self.phase, Phase::Settle) {
            self.phase = Phase::Cancelled;
            return;
        }
        self.error = Some(HistoryReadError::Interrupted);
        self.restore_at.get_or_insert(now);
        if !matches!(self.phase, Phase::Probe | Phase::UndoProbe) {
            // Keep `action_at`: an upward tick still in flight must settle before release.
            self.phase = Phase::Restore;
        }
    }

    /// Observe one frame, using the event loop's monotonic clock. Synchronized output never
    /// counts as quiet. The returned wheel action must execute before the next observation.
    pub fn observe(&mut self, frame: HistoryReadFrame, now: Instant) -> HistoryReadStep {
        if matches!(self.phase, Phase::Cancelled) {
            return self.finish(Err(HistoryReadError::Interrupted));
        }
        if matches!(self.phase, Phase::Finished) {
            return HistoryReadStep::Finished(Err(HistoryReadError::Unavailable));
        }
        if !self.observe_frame(frame, now) {
            return self.finish(Err(HistoryReadError::RestoreUnproven));
        }
        if let Some(end) = self.expired(now) {
            return end;
        }
        if !self.quiet(now) {
            return HistoryReadStep::Wait;
        }
        self.advance(now)
    }

    fn observe_frame(&mut self, frame: HistoryReadFrame, now: Instant) -> bool {
        if frame.viewport != self.original.viewport || frame.rows.len() != self.original.rows.len()
        {
            return false;
        }
        if frame.rows != self.observed.rows {
            self.changed_at = now;
            self.changed = true;
        }
        if frame.synchronized != self.observed.synchronized {
            self.changed_at = now;
        }
        self.observed = frame;
        if self.observed.synchronized {
            self.changed_at = now;
        }
        true
    }

    fn expired(&mut self, now: Instant) -> Option<HistoryReadStep> {
        if let Some(started) = self.restore_at {
            return (now.saturating_duration_since(started) >= RESTORATION)
                .then(|| self.finish(Err(HistoryReadError::RestoreUnproven)));
        }
        (now.saturating_duration_since(self.started) >= TRAVERSAL)
            .then(|| self.restore(HistoryReadError::Limit, now))
    }

    fn quiet(&self, now: Instant) -> bool {
        !self.observed.synchronized && now.saturating_duration_since(self.changed_at) >= QUIET
    }

    fn advance(&mut self, now: Instant) -> HistoryReadStep {
        match self.phase {
            Phase::Settle => {
                // A screen that changed while initially settling is the baseline actually probed.
                self.original = self.observed.clone();
                self.page = self.observed.rows.clone();
                self.content = self.page.iter().cloned().collect();
                self.scroll(Phase::Probe, false, now)
            }
            Phase::Probe => self.probe(now),
            Phase::UndoProbe => self.undo_probe(now),
            Phase::Harvest => self.harvest(now),
            Phase::Restore => self.restore_step(now),
            _ => HistoryReadStep::Wait,
        }
    }

    fn probe(&mut self, now: Instant) -> HistoryReadStep {
        if self.observed.rows != self.original.rows {
            self.error.get_or_insert(HistoryReadError::NotAtBottom);
            self.restore_at = Some(now);
            return self.scroll(Phase::UndoProbe, true, now);
        }
        if self.action_settled(now) {
            if let Some(error) = self.error {
                return self.finish(Err(error));
            }
            return self.scroll(Phase::Harvest, true, now);
        }
        HistoryReadStep::Wait
    }

    fn undo_probe(&mut self, now: Instant) -> HistoryReadStep {
        if self.observed.rows == self.original.rows {
            return self.finish(Err(self.error.unwrap_or(HistoryReadError::NotAtBottom)));
        }
        if self.action_settled(now) {
            return self.finish(Err(HistoryReadError::RestoreUnproven));
        }
        HistoryReadStep::Wait
    }

    fn action_settled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.action_at) >= RESPONSE
    }

    fn harvest(&mut self, now: Instant) -> HistoryReadStep {
        if !self.changed || self.observed.rows == self.page {
            if !self.action_settled(now) {
                return HistoryReadStep::Wait;
            }
            if self.pages == 0 {
                return self.finish(Err(HistoryReadError::Unavailable));
            }
            self.phase = Phase::Restore;
            self.restore_at = Some(now);
            return self.scroll(Phase::Restore, false, now);
        }
        if !self.merge_current_page() {
            return self.restore(HistoryReadError::AmbiguousOverlap, now);
        }
        self.pages += 1;
        if self.pages >= MAX_PAGES
            || self
                .content
                .iter()
                .map(|row| row.text.len() + 1)
                .sum::<usize>()
                > MAX_BYTES
        {
            return self.restore(HistoryReadError::Limit, now);
        }
        self.scroll(Phase::Harvest, true, now)
    }

    fn merge_current_page(&mut self) -> bool {
        let current = &self.observed.rows;
        let Some(crop) = self.crop.or_else(|| unique_crop(&self.page, current)) else {
            return false;
        };
        let (top, bottom) = crop;
        let end = current.len().saturating_sub(bottom);
        let Some(added) = prepend_rows(&self.page[top..end], &current[top..end]) else {
            return false;
        };
        if self.crop.is_none() {
            self.content = self.page[top..end].iter().cloned().collect();
            self.crop = Some(crop);
        }
        for row in added.iter().rev() {
            self.content.push_front(row.clone());
        }
        self.page = current.clone();
        true
    }

    fn restore(&mut self, error: HistoryReadError, now: Instant) -> HistoryReadStep {
        self.error = Some(error);
        self.restore_at.get_or_insert(now);
        match self.phase {
            Phase::Settle => return self.finish(Err(error)),
            Phase::Probe => return self.probe(now),
            Phase::UndoProbe => return self.undo_probe(now),
            _ => {}
        }
        self.phase = Phase::Restore;
        self.restore_step(now)
    }

    fn restore_step(&mut self, now: Instant) -> HistoryReadStep {
        if self.observed.rows == self.original.rows
            && (!self.pending_up || self.action_settled(now))
        {
            return match self.error {
                Some(error) => self.finish(Err(error)),
                None => {
                    let mut text = String::new();
                    for row in &self.content {
                        text.push_str(&row.text);
                        if !row.soft_wrapped {
                            text.push('\n');
                        }
                    }
                    self.finish(Ok(text))
                }
            };
        }
        if self.changed || self.action_settled(now) {
            return self.scroll(Phase::Restore, false, now);
        }
        HistoryReadStep::Wait
    }

    fn scroll(&mut self, phase: Phase, up: bool, now: Instant) -> HistoryReadStep {
        self.phase = phase;
        self.action_at = now;
        self.changed_at = now;
        self.changed = false;
        self.pending_up = up;
        if up {
            HistoryReadStep::ScrollUp
        } else {
            HistoryReadStep::ScrollDown
        }
    }

    fn finish(&mut self, result: Result<String, HistoryReadError>) -> HistoryReadStep {
        self.phase = Phase::Finished;
        HistoryReadStep::Finished(result)
    }
}

/// Choose the pinned header/footer crop only when exactly one interpretation reconstructs the
/// page transition. Equal edge rows may be real repeated transcript instead of chrome, so any
/// second viable `(top, bottom)` crop makes the transition ambiguous.
fn unique_crop(previous: &[HistoryReadRow], current: &[HistoryReadRow]) -> Option<(usize, usize)> {
    let len = current.len();
    if previous.len() != len {
        return None;
    }
    let max_top = previous
        .iter()
        .zip(current)
        .take_while(|(a, b)| a == b)
        .count();
    let max_bottom = previous
        .iter()
        .rev()
        .zip(current.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let mut found = None;
    for top in 0..=max_top {
        for bottom in 0..=max_bottom {
            if top + bottom >= len {
                continue;
            }
            let end = len - bottom;
            if prepend_rows(&previous[top..end], &current[top..end]).is_some() {
                if found.is_some() {
                    return None;
                }
                found = Some((top, bottom));
            }
        }
    }
    found
}

/// Accept only a unique offset with at least three overlapping rows and a unique nonblank anchor.
/// Repeated rows cannot supply enough evidence on their own.
fn prepend_rows<'a>(
    previous: &[HistoryReadRow],
    current: &'a [HistoryReadRow],
) -> Option<&'a [HistoryReadRow]> {
    if previous.len() != current.len() {
        return None;
    }
    let mut found = None;
    for shift in anchor_offsets(previous, current) {
        let overlap = current.len() - shift;
        if current[shift..] != previous[..overlap] {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(&current[..shift]);
    }
    found
}

fn unique_positions(rows: &[HistoryReadRow]) -> HashMap<&HistoryReadRow, Option<usize>> {
    let mut positions = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        positions
            .entry(row)
            .and_modify(|position| *position = None)
            .or_insert(Some(index));
    }
    positions
}

fn anchor_offsets(previous: &[HistoryReadRow], current: &[HistoryReadRow]) -> BTreeSet<usize> {
    let previous = unique_positions(previous);
    let mut offsets = BTreeSet::new();
    for (row, position) in unique_positions(current) {
        let Some(current_index) = position else {
            continue;
        };
        let Some(Some(previous_index)) = previous.get(row) else {
            continue;
        };
        if row.text.trim().is_empty() || current_index <= *previous_index {
            continue;
        }
        let shift = current_index - previous_index;
        if current.len() - shift >= MIN_OVERLAP {
            offsets.insert(shift);
        }
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(text: impl Into<Arc<str>>) -> HistoryReadRow {
        HistoryReadRow {
            text: text.into(),
            soft_wrapped: false,
        }
    }

    fn frame(start: usize) -> HistoryReadFrame {
        let mut rows = vec![row("header")];
        rows.extend((start..start + 6).map(|index| row(format!("row {index} 🌷"))));
        rows.push(row("footer"));
        HistoryReadFrame {
            viewport: TerminalViewport { cols: 40, rows: 8 },
            rows,
            synchronized: false,
        }
    }

    // The stand-in is the external application's scroll boundary, never the reconstruction.
    fn drive(start: usize, interrupt: bool) -> (Result<String, HistoryReadError>, usize) {
        drive_with_sync(start, interrupt, false)
    }

    fn drive_with_sync(
        start: usize,
        interrupt: bool,
        synchronized_probe: bool,
    ) -> (Result<String, HistoryReadError>, usize) {
        let mut position = start;
        let mut now = Instant::now();
        let mut reader = AltScreenHistoryReader::new(frame(position), now).unwrap();
        let mut interrupted = false;
        let mut sync_pending = false;
        let mut probed = false;
        for _ in 0..300 {
            now += Duration::from_millis(250);
            let mut observation = frame(position);
            observation.synchronized = std::mem::take(&mut sync_pending);
            match reader.observe(observation, now) {
                HistoryReadStep::Wait => {}
                HistoryReadStep::ScrollUp => {
                    position = position.saturating_sub(2);
                    if interrupt && !interrupted {
                        reader.interrupt(now);
                        interrupted = true;
                    }
                }
                HistoryReadStep::ScrollDown => {
                    position = (position + 2).min(6);
                    sync_pending = synchronized_probe && !probed;
                    probed = true;
                }
                HistoryReadStep::Finished(result) => return (result, position),
            }
        }
        panic!("reader did not terminate")
    }

    #[test]
    fn harvests_overlapping_pages_and_proves_bottom_restoration() {
        let (result, position) = drive(6, false);
        assert_eq!(position, 6);
        let expected: String = (0..12).map(|index| format!("row {index} 🌷\n")).collect();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn synchronized_redraw_with_identical_rows_is_not_mistaken_for_a_scroll() {
        let (result, position) = drive_with_sync(6, false, true);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(position, 6);
    }

    #[test]
    fn manually_scrolled_view_is_undone_and_refused() {
        let (result, position) = drive(2, false);
        assert_eq!(result, Err(HistoryReadError::NotAtBottom));
        assert_eq!(position, 2);
    }

    #[test]
    fn interruption_restores_before_releasing_the_lease() {
        let (result, position) = drive(6, true);
        assert_eq!(result, Err(HistoryReadError::Interrupted));
        assert_eq!(position, 6);
    }

    #[test]
    fn interruption_during_the_bottom_probe_preserves_the_starting_position() {
        for start in [2, 6] {
            let mut position = start;
            let mut now = Instant::now();
            let mut reader = AltScreenHistoryReader::new(frame(position), now).unwrap();
            let mut interrupted = false;
            let mut result = None;
            for _ in 0..40 {
                now += Duration::from_millis(250);
                match reader.observe(frame(position), now) {
                    HistoryReadStep::ScrollDown => {
                        position = (position + 2).min(6);
                        if !interrupted {
                            reader.interrupt(now);
                            interrupted = true;
                        }
                    }
                    HistoryReadStep::ScrollUp => position = position.saturating_sub(2),
                    HistoryReadStep::Finished(end) => {
                        result = Some(end);
                        break;
                    }
                    _ => {}
                }
            }
            assert_eq!(result, Some(Err(HistoryReadError::Interrupted)));
            assert_eq!(position, start);
        }
    }

    #[test]
    fn the_terminal_frame_reports_real_modes_and_unicode_cells() {
        let mut screen = super::super::TerminalScreen::new(8, 40, 0);
        assert!(!screen.is_alternate_screen());
        screen.process_bytes(b"\x1b[?1049h\x1b[?1000h\x1b[?1006h");
        screen.process_bytes("日本語 🌷".as_bytes());
        assert!(screen.is_alternate_screen());
        let frame = screen.history_read_frame();
        assert_eq!(frame.viewport, TerminalViewport { rows: 8, cols: 40 });
        assert!(frame.rows[0].text.contains("日本語 🌷"));
        screen.process_bytes(b"\x1b[?2026hwaiting");
        assert!(screen.history_read_frame().synchronized);
        screen.process_bytes(b"\x1b[?2026l\x1b[?1049l");
        assert!(!screen.is_alternate_screen());
    }

    #[test]
    fn repeated_edge_rows_are_not_silently_taken_as_chrome() {
        let previous = ["same", "B", "C", "D", "E", "F"].map(row);
        let current = ["same", "same", "B", "C", "D", "E"].map(row);
        assert!(unique_crop(&previous, &current).is_none());
        let previous = ["head", "B", "C", "D", "E", "foot"].map(row);
        let current = ["head", "A", "B", "C", "D", "foot"].map(row);
        assert_eq!(unique_crop(&previous, &current), Some((1, 1)));
    }

    #[test]
    fn interruption_waits_for_an_in_flight_scroll_up() {
        let mut now = Instant::now();
        let mut reader = AltScreenHistoryReader::new(frame(6), now).unwrap();
        let mut step = HistoryReadStep::Wait;
        for _ in 0..4 {
            now += Duration::from_millis(250);
            step = reader.observe(frame(6), now);
            if step == HistoryReadStep::ScrollUp {
                break;
            }
        }
        assert_eq!(step, HistoryReadStep::ScrollUp);
        reader.interrupt(now);
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(frame(6), now), HistoryReadStep::Wait);
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(frame(6), now), HistoryReadStep::Wait);
        // The response finally lands; it must be observed and undone, not outrun.
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(frame(4), now), HistoryReadStep::Wait);
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(frame(4), now), HistoryReadStep::ScrollDown);
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(frame(6), now), HistoryReadStep::Wait);
        now += Duration::from_millis(250);
        assert_eq!(
            reader.observe(frame(6), now),
            HistoryReadStep::Finished(Err(HistoryReadError::Interrupted))
        );
    }

    #[test]
    fn duplicate_or_blank_rows_cannot_establish_an_anchor() {
        let repeated = vec![row("same"); 8];
        assert!(prepend_rows(&repeated, &repeated).is_none());
        let blank = vec![row(""); 8];
        assert!(prepend_rows(&blank, &blank).is_none());
    }

    #[test]
    fn ambiguous_and_nonoverlapping_changes_are_refused() {
        let previous = ["a", "b", "c", "d", "e", "f"].map(row);
        let changed = ["new", "different", "page", "x", "y", "z"].map(row);
        assert!(prepend_rows(&previous, &changed).is_none());
        let weak = ["new", "newer", "newest", "extra", "a", "b"].map(row);
        assert!(prepend_rows(&previous, &weak).is_none());
    }

    #[test]
    fn overlapping_unicode_rows_retain_their_soft_wrap_metadata() {
        let mut previous = ["日本", "語🌷", "three", "four", "five", "six"].map(row);
        previous[0].soft_wrapped = true;
        let current = [
            row("first"),
            previous[0].clone(),
            previous[1].clone(),
            previous[2].clone(),
            previous[3].clone(),
            previous[4].clone(),
        ];
        assert_eq!(prepend_rows(&previous, &current).unwrap(), &[row("first")]);
    }

    #[test]
    fn resize_and_restoration_timeout_never_return_text() {
        let now = Instant::now();
        let mut reader = AltScreenHistoryReader::new(frame(6), now).unwrap();
        let mut resized = frame(6);
        resized.viewport.cols = 80;
        assert_eq!(
            reader.observe(resized, now),
            HistoryReadStep::Finished(Err(HistoryReadError::RestoreUnproven))
        );
        let mut reader = AltScreenHistoryReader::new(frame(6), now).unwrap();
        assert_eq!(
            reader.observe(frame(6), now + QUIET),
            HistoryReadStep::ScrollDown
        );
        let moved_at = now + QUIET + RESPONSE;
        assert_eq!(
            reader.observe(frame(6), moved_at),
            HistoryReadStep::ScrollUp
        );
        reader.interrupt(moved_at);
        assert_eq!(
            reader.observe(frame(4), moved_at + RESTORATION),
            HistoryReadStep::Finished(Err(HistoryReadError::RestoreUnproven))
        );
    }

    #[test]
    fn synchronized_frames_do_not_settle_or_issue_scrolls() {
        let now = Instant::now();
        let mut pending = frame(6);
        pending.synchronized = true;
        let mut reader = AltScreenHistoryReader::new(pending.clone(), now).unwrap();
        assert_eq!(
            reader.observe(pending, now + Duration::from_secs(1)),
            HistoryReadStep::Wait
        );
    }

    #[test]
    fn overlap_offsets_are_correct_across_duplicate_row_corpora() {
        let mut seed = 42u64;
        for _ in 0..2000 {
            let rows: Vec<_> = (0..16)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    row(format!("{}", (seed >> 32) % 9))
                })
                .collect();
            for shift in 1..=4 {
                let previous = &rows[4..12];
                let current = &rows[4 - shift..12 - shift];
                if let Some(added) = prepend_rows(previous, current) {
                    assert_eq!(added, &rows[4 - shift..4]);
                }
            }
        }
    }
}
