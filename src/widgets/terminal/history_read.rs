//! Bounded, non-blocking history reconstruction for an idle alternate-screen application.
//!
//! The owner feeds settled screen observations and executes returned wheel actions. It must hold
//! an exclusive input lease, prohibit raw attachments, and interrupt on input, resize, lifecycle
//! change, or unrelated output. No result is released until the original bottom viewport returns.
//!
//! Applications decorate a scrolled transcript: a scrollbar thumb moves, a "back to bottom" notice
//! covers a row, a status line changes. Pages are therefore compared without scrollbars, and
//! aligned on the rows that moved rather than on the whole viewport.
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthStr;

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

/// Reconstructed history, returned once the original viewport is restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRead {
    /// Rendered rows, oldest first, without scrollbars. Pinned chrome above the transcript is left
    /// out; rows between the transcript and a pinned footer may remain.
    pub text: String,
    /// Scrolling reached the top of the transcript. When `false`, a traversal bound or a page that
    /// could not be aligned ended the read, and `text` holds the newest rows that were read.
    pub complete: bool,
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
    /// The first scrolled page could not be aligned with the bottom page.
    #[error("history pages do not have a unique strong overlap")]
    AmbiguousOverlap,
    /// No scrollable history was observed.
    #[error("scrollable application history is unavailable")]
    Unavailable,
    /// A traversal bound was reached before any page was read.
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
    /// Send this many upward wheel ticks, in the application's active mouse protocol.
    ScrollUp(u16),
    /// Send this many downward wheel ticks, in the application's active mouse protocol.
    ScrollDown(u16),
    /// The read ended. Only `Ok` contains text, after restoration was proven.
    Finished(Result<HistoryRead, HistoryReadError>),
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
    /// The original rows without scrollbars, which restoration must reproduce.
    original_rows: Vec<HistoryReadRow>,
    observed: HistoryReadFrame,
    observed_rows: Vec<HistoryReadRow>,
    /// The last page merged into `content`, without scrollbars.
    page: Vec<HistoryReadRow>,
    content: VecDeque<HistoryReadRow>,
    phase: Phase,
    started: Instant,
    action_at: Instant,
    changed_at: Instant,
    restore_at: Option<Instant>,
    /// The last action sent was an upward scroll whose response may still be in flight.
    pending_up: bool,
    changed: bool,
    pages: usize,
    /// The transcript's first row, once a page proved it.
    band_top: Option<usize>,
    /// Ticks in the next upward scroll, sized from how far one tick moves the transcript.
    ticks: u16,
    /// Ticks in the upward scroll whose page is awaited.
    sent_ticks: u16,
    /// Upward ticks sent while harvesting and not yet sent back down.
    up_ticks: u32,
    complete: bool,
    error: Option<HistoryReadError>,
}

const QUIET: Duration = Duration::from_millis(60);
const RESPONSE: Duration = Duration::from_millis(600);
const TRAVERSAL: Duration = Duration::from_secs(10);
const RESTORATION: Duration = Duration::from_secs(3);
const MAX_PAGES: usize = 256;
const MAX_BYTES: usize = 1024 * 1024;
const MIN_OVERLAP: usize = 3;
const MAX_TICKS: u16 = 16;
/// Ticks in one restoring burst. Larger than an upward scroll, so restoration is never slower
/// than the traversal it undoes.
const RESTORE_TICKS: u16 = 64;
/// A scrollbar glyph counts as one only within this many columns of the right edge.
const SCROLLBAR_MARGIN: usize = 3;

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
        let rows = without_scrollbars(&frame);
        Ok(Self {
            original_rows: rows.clone(),
            observed_rows: rows.clone(),
            page: rows,
            content: VecDeque::new(),
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
            band_top: None,
            ticks: 1,
            sent_ticks: 0,
            up_ticks: 0,
            complete: true,
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
            // Keep `action_at`: an upward scroll still in flight must settle before release.
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
        let rows = without_scrollbars(&frame);
        if rows != self.observed_rows {
            self.changed_at = now;
            self.changed = true;
        }
        if frame.synchronized != self.observed.synchronized {
            self.changed_at = now;
        }
        self.observed = frame;
        self.observed_rows = rows;
        if self.observed.synchronized {
            self.changed_at = now;
        }
        true
    }

    fn expired(&mut self, now: Instant) -> Option<HistoryReadStep> {
        if let Some(started) = self.restore_at {
            // Coming back may take as long as going up did.
            let allowed = RESTORATION + started.saturating_duration_since(self.started);
            return (now.saturating_duration_since(started) >= allowed)
                .then(|| self.finish(Err(HistoryReadError::RestoreUnproven)));
        }
        (now.saturating_duration_since(self.started) >= TRAVERSAL)
            .then(|| self.stop_early(HistoryReadError::Limit, now))
    }

    fn quiet(&self, now: Instant) -> bool {
        !self.observed.synchronized && now.saturating_duration_since(self.changed_at) >= QUIET
    }

    fn advance(&mut self, now: Instant) -> HistoryReadStep {
        match self.phase {
            Phase::Settle => {
                // A screen that changed while initially settling is the baseline actually probed.
                self.original = self.observed.clone();
                self.original_rows = self.observed_rows.clone();
                self.page = self.observed_rows.clone();
                self.scroll(Phase::Probe, false, 1, now)
            }
            Phase::Probe => self.probe(now),
            Phase::UndoProbe => self.undo_probe(now),
            Phase::Harvest => self.harvest(now),
            Phase::Restore => self.restore_step(now),
            _ => HistoryReadStep::Wait,
        }
    }

    fn probe(&mut self, now: Instant) -> HistoryReadStep {
        if self.observed_rows != self.original_rows {
            self.error.get_or_insert(HistoryReadError::NotAtBottom);
            self.restore_at = Some(now);
            return self.scroll(Phase::UndoProbe, true, 1, now);
        }
        if self.action_settled(now) {
            if let Some(error) = self.error {
                return self.finish(Err(error));
            }
            return self.scroll_up(now);
        }
        HistoryReadStep::Wait
    }

    fn undo_probe(&mut self, now: Instant) -> HistoryReadStep {
        if self.observed_rows == self.original_rows {
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
        if !self.changed || self.observed_rows == self.page {
            if !self.action_settled(now) {
                return HistoryReadStep::Wait;
            }
            if self.pages == 0 {
                return self.restore(HistoryReadError::Unavailable, now);
            }
            // The transcript stopped moving: its top has been read.
            return self.start_restore(now);
        }
        let Some(alignment) = align(&self.page, &self.observed_rows, self.band_top) else {
            return self.stop_early(HistoryReadError::AmbiguousOverlap, now);
        };
        if self.pages == 0 {
            self.content = self.page[alignment.top..alignment.bottom]
                .iter()
                .cloned()
                .collect();
            while self.content.back().is_some_and(is_blank) {
                self.content.pop_back();
            }
        }
        let added = &self.observed_rows[alignment.top..alignment.top + alignment.shift];
        for row in added.iter().rev() {
            self.content.push_front(row.clone());
        }
        self.band_top = Some(alignment.top);
        self.pages += 1;
        if self.pages >= MAX_PAGES
            || self
                .content
                .iter()
                .map(|row| row.text.len() + 1)
                .sum::<usize>()
                > MAX_BYTES
        {
            return self.stop_early(HistoryReadError::Limit, now);
        }
        // Aim for half the moving region per scroll, which keeps a strong overlap.
        let per_tick = (alignment.shift / usize::from(self.sent_ticks.max(1))).max(1);
        let ticks = (alignment.span / 2 / per_tick).max(1);
        self.ticks = u16::try_from(ticks).unwrap_or(MAX_TICKS).min(MAX_TICKS);
        self.page = self.observed_rows.clone();
        self.scroll_up(now)
    }

    /// End the traversal. Rows already read are still returned, as incomplete history.
    fn stop_early(&mut self, error: HistoryReadError, now: Instant) -> HistoryReadStep {
        if self.pages == 0 {
            return self.restore(error, now);
        }
        self.complete = false;
        self.start_restore(now)
    }

    fn start_restore(&mut self, now: Instant) -> HistoryReadStep {
        self.restore_at.get_or_insert(now);
        self.phase = Phase::Restore;
        self.restore_step(now)
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
        if self.observed_rows == self.original_rows
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
                    let complete = self.complete;
                    self.finish(Ok(HistoryRead { text, complete }))
                }
            };
        }
        if self.changed || self.action_settled(now) {
            // Applications cap how far one burst moves, so return in bounded bursts. Once the
            // ticks sent up are spent, keep going in full bursts: the probe proved that ticks
            // at the bottom change nothing.
            let ticks = match self.up_ticks {
                0 => RESTORE_TICKS,
                pending => u16::try_from(pending)
                    .unwrap_or(RESTORE_TICKS)
                    .min(RESTORE_TICKS),
            };
            self.up_ticks = self.up_ticks.saturating_sub(u32::from(ticks));
            return self.scroll(Phase::Restore, false, ticks, now);
        }
        HistoryReadStep::Wait
    }

    fn scroll_up(&mut self, now: Instant) -> HistoryReadStep {
        self.sent_ticks = self.ticks;
        self.up_ticks = self.up_ticks.saturating_add(u32::from(self.ticks));
        self.scroll(Phase::Harvest, true, self.ticks, now)
    }

    fn scroll(&mut self, phase: Phase, up: bool, ticks: u16, now: Instant) -> HistoryReadStep {
        self.phase = phase;
        self.action_at = now;
        self.changed_at = now;
        self.changed = false;
        self.pending_up = up;
        if up {
            HistoryReadStep::ScrollUp(ticks)
        } else {
            HistoryReadStep::ScrollDown(ticks)
        }
    }

    fn finish(&mut self, result: Result<HistoryRead, HistoryReadError>) -> HistoryReadStep {
        self.phase = Phase::Finished;
        HistoryReadStep::Finished(result)
    }
}

fn without_scrollbars(frame: &HistoryReadFrame) -> Vec<HistoryReadRow> {
    frame
        .rows
        .iter()
        .map(|row| without_scrollbar(row, frame.viewport.cols))
        .collect()
}

/// Drop a lone track or thumb glyph at the right edge. The decision depends only on the row
/// itself, so a transcript row compares the same wherever the thumb happens to be.
fn without_scrollbar(row: &HistoryReadRow, cols: u16) -> HistoryReadRow {
    let text = row.text.trim_end();
    if let Some((index, glyph)) = text.char_indices().next_back() {
        let before = &text[..index];
        if is_scrollbar_glyph(glyph)
            && (before.is_empty() || before.ends_with(' '))
            && before.width() + SCROLLBAR_MARGIN >= usize::from(cols)
        {
            return HistoryReadRow {
                text: Arc::from(before.trim_end()),
                soft_wrapped: row.soft_wrapped,
            };
        }
    }
    if text.len() == row.text.len() {
        return row.clone();
    }
    HistoryReadRow {
        text: Arc::from(text),
        soft_wrapped: row.soft_wrapped,
    }
}

fn is_scrollbar_glyph(glyph: char) -> bool {
    matches!(
        glyph,
        '│' | '┃'
            | '║'
            | '╎'
            | '╏'
            | '┆'
            | '┇'
            | '┊'
            | '┋'
            | '╵'
            | '╷'
            | '╹'
            | '╻'
            | '╽'
            | '╿'
            | '▏'
            | '▎'
            | '▍'
            | '▌'
            | '▋'
            | '▊'
            | '▉'
            | '█'
            | '▐'
            | '▕'
            | '▀'
            | '▄'
            | '░'
            | '▒'
            | '▓'
            | '▲'
            | '▼'
    )
}

fn is_blank(row: &HistoryReadRow) -> bool {
    row.text.trim().is_empty()
}

/// How the transcript moved between two pages, in row indices of the earlier page.
#[derive(Debug, PartialEq, Eq)]
struct Alignment {
    /// Rows the transcript moved down, which equals the rows revealed above it.
    shift: usize,
    /// The first transcript row. Rows above it are chrome the scroll did not move.
    top: usize,
    /// End of the transcript on the earlier page: the first row proven to be a pinned footer.
    bottom: usize,
    /// Height of the region that moved.
    span: usize,
}

/// Find the one shift that explains a scroll, then where the transcript starts and ends.
///
/// Rows identical in place carry no evidence for the shift. The rest must mostly agree at that
/// shift, through a row unique to both pages, and no other shift may be plausible: repeated rows
/// alone cannot tell a small scroll from a larger one.
///
/// A row identical in place is not thereby chrome: repeated transcript can sit at the same
/// position after a scroll. Boundaries rest on what the shift explains, a row explained both ways
/// is never dropped, and `known_top`, the top proven by an earlier page, settles such a row above
/// the transcript. Without it the pages are refused rather than guessed.
fn align(
    previous: &[HistoryReadRow],
    current: &[HistoryReadRow],
    known_top: Option<usize>,
) -> Option<Alignment> {
    let len = previous.len();
    if current.len() != len || len < MIN_OVERLAP {
        return None;
    }
    let pinned: Vec<bool> = previous.iter().zip(current).map(|(a, b)| a == b).collect();
    let (shift, first, last) = shift_between(previous, current, &pinned)?;
    let moved = |index: usize| index + shift < len && previous[index] == current[index + shift];
    let top = band_top(first, &pinned, moved, known_top)?;
    // A footer row is pinned, and differs from the transcript row the shift brings to its place.
    let bottom = (last + 1..len)
        .find(|&index| {
            pinned[index]
                && !is_blank(&previous[index])
                && index >= top + shift
                && current[index] != previous[index - shift]
        })
        .unwrap_or(len);
    Some(Alignment {
        shift,
        top,
        bottom,
        span: last - top + 1 + shift,
    })
}

/// The unique plausible shift, with the first and last earlier-page rows that support it.
fn shift_between(
    previous: &[HistoryReadRow],
    current: &[HistoryReadRow],
    pinned: &[bool],
) -> Option<(usize, usize, usize)> {
    let len = previous.len();
    let previous_unique = unique_positions(previous);
    let current_unique = unique_positions(current);
    let mut found = None;
    for shift in 1..=len - MIN_OVERLAP {
        let mut comparable = 0;
        let mut matched = 0;
        let mut anchored = false;
        let mut first = None;
        let mut last = 0;
        for index in 0..len - shift {
            let (before, after) = (&previous[index], &current[index + shift]);
            if pinned[index] || pinned[index + shift] || is_blank(before) || is_blank(after) {
                continue;
            }
            comparable += 1;
            if before != after {
                continue;
            }
            matched += 1;
            first.get_or_insert(index);
            last = index;
            anchored |= previous_unique.get(before) == Some(&Some(index))
                && current_unique.get(after) == Some(&Some(index + shift));
        }
        if !anchored || matched < MIN_OVERLAP || matched * 2 < comparable {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some((shift, first.expect("matched rows"), last));
    }
    found
}

/// Walk up from the first supporting row while rows moved with the transcript, blank ones
/// included. A row the scroll did not move is chrome. A row that both moved and stayed in place
/// is ambiguous: only a top proven by an earlier page decides it.
fn band_top(
    first: usize,
    pinned: &[bool],
    moved: impl Fn(usize) -> bool,
    known_top: Option<usize>,
) -> Option<usize> {
    let mut top = first;
    while top > 0 {
        let row = top - 1;
        if !moved(row) {
            break;
        }
        if pinned[row] {
            return match known_top {
                Some(known) if known <= row => Some(known).filter(|_| (known..row).all(&moved)),
                Some(known) if known == top => Some(known),
                _ => None,
            };
        }
        top = row;
    }
    Some(top)
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

#[cfg(test)]
mod tests {
    use super::*;

    const COLS: u16 = 40;

    fn row(text: impl Into<Arc<str>>) -> HistoryReadRow {
        HistoryReadRow {
            text: text.into(),
            soft_wrapped: false,
        }
    }

    /// Stand-in for the external application at the scroll boundary: a pinned header, a
    /// transcript, a status row and a pinned input row. The reconstruction under test is the
    /// production reader.
    #[derive(Clone)]
    struct App {
        lines: Vec<String>,
        rows: usize,
        per_tick: usize,
        offset: usize,
        /// A one-row thumb at the right edge that follows the scroll position.
        scrollbar: bool,
        /// The scrollbar only appears once scrolled, and stays.
        lingering_scrollbar: bool,
        touched: bool,
        /// A scrolled transcript has its last row covered and its status row changed.
        decorations: bool,
        /// One downward burst moves at most this many lines, however many ticks it carries.
        down_cap: usize,
    }

    impl App {
        fn new(lines: usize, per_tick: usize) -> Self {
            Self::with_lines((0..lines).map(|index| format!("line {index:03}")), per_tick)
        }

        fn with_lines(lines: impl IntoIterator<Item = String>, per_tick: usize) -> Self {
            Self {
                lines: lines.into_iter().collect(),
                rows: 12,
                per_tick,
                offset: 0,
                scrollbar: false,
                lingering_scrollbar: false,
                touched: false,
                decorations: false,
                down_cap: usize::MAX,
            }
        }

        fn body(&self) -> usize {
            self.rows - 3
        }

        fn max_offset(&self) -> usize {
            self.lines.len().saturating_sub(self.body())
        }

        fn frame(&self) -> HistoryReadFrame {
            let body = self.body();
            let end = self.lines.len() - self.offset;
            let mut visible = vec![String::new(); body.saturating_sub(end)];
            visible.extend_from_slice(&self.lines[end.saturating_sub(body)..end]);
            if self.decorations && self.offset > 0 {
                *visible.last_mut().unwrap() = "  ↓ back to bottom".into();
            }
            let show_bar = self.scrollbar || (self.lingering_scrollbar && self.touched);
            let thumb = (body - 1) * (self.max_offset() - self.offset) / self.max_offset().max(1);
            let mut rows = vec![row("pinned header")];
            rows.extend(visible.into_iter().enumerate().map(|(index, text)| {
                if show_bar && index == thumb {
                    let pad = usize::from(COLS) - 1 - text.width();
                    row(format!("{text}{}█", " ".repeat(pad)))
                } else {
                    row(text)
                }
            }));
            rows.push(row(if self.decorations && self.offset > 0 {
                "scrolled · esc"
            } else {
                "ready · ? help"
            }));
            rows.push(row("> input"));
            HistoryReadFrame {
                viewport: TerminalViewport {
                    cols: COLS,
                    rows: u16::try_from(self.rows).unwrap(),
                },
                rows,
                synchronized: false,
            }
        }

        fn wheel(&mut self, up: bool, ticks: u16) {
            self.touched = true;
            let lines = usize::from(ticks) * self.per_tick;
            self.offset = if up {
                (self.offset + lines).min(self.max_offset())
            } else {
                self.offset.saturating_sub(lines.min(self.down_cap))
            };
        }
    }

    fn drive(app: &mut App, interrupt: bool) -> Result<HistoryRead, HistoryReadError> {
        let mut now = Instant::now();
        let mut reader = AltScreenHistoryReader::new(app.frame(), now).unwrap();
        let mut interrupted = false;
        for _ in 0..2000 {
            now += Duration::from_millis(100);
            match reader.observe(app.frame(), now) {
                HistoryReadStep::Wait => {}
                HistoryReadStep::ScrollUp(ticks) => {
                    app.wheel(true, ticks);
                    if interrupt && !interrupted && app.offset > 0 {
                        reader.interrupt(now);
                        interrupted = true;
                    }
                }
                HistoryReadStep::ScrollDown(ticks) => app.wheel(false, ticks),
                HistoryReadStep::Finished(result) => return result,
            }
        }
        panic!("reader did not terminate")
    }

    fn transcript(app: &App) -> String {
        app.lines.iter().map(|line| format!("{line}\n")).collect()
    }

    #[test]
    fn harvests_the_whole_transcript_and_restores_the_bottom() {
        for per_tick in [1, 3] {
            let mut app = App::new(60, per_tick);
            let read = drive(&mut app, false).unwrap();
            assert_eq!(app.offset, 0);
            assert!(read.complete);
            // The status row never changed, so it is pinned chrome like the input row.
            assert_eq!(read.text, transcript(&app));
        }
    }

    #[test]
    fn scrollbars_covered_rows_and_changed_status_still_align() {
        for per_tick in [1, 3] {
            let mut app = App::new(60, per_tick);
            app.scrollbar = true;
            app.decorations = true;
            let read = drive(&mut app, false).unwrap();
            assert_eq!(app.offset, 0);
            assert!(read.complete, "{read:?}");
            assert!(read.text.starts_with(&transcript(&app)), "{}", read.text);
            assert!(!read.text.contains('█'), "{}", read.text);
        }
    }

    #[test]
    fn repeated_and_blank_transcript_rows_are_reconstructed_exactly() {
        let unique = (0..40).map(|index| format!("line {index:03}"));
        let cases: [Vec<String>; 2] = [
            // The bottom pair repeats, so it sits in place after a two-line scroll.
            unique
                .clone()
                .chain(["g", "h", "g", "h"].map(String::from))
                .collect(),
            // The first visible transcript row is blank.
            unique
                .clone()
                .take(31)
                .chain([String::new()])
                .chain(unique.clone().skip(31).take(8))
                .collect(),
        ];
        for lines in cases {
            let mut app = App::with_lines(lines, 2);
            let read = drive(&mut app, false).unwrap();
            assert_eq!(app.offset, 0);
            assert!(read.complete);
            assert_eq!(read.text, transcript(&app));
        }
    }

    #[test]
    fn a_downward_burst_the_application_caps_still_restores_the_bottom() {
        let mut app = App::new(300, 1);
        app.rows = 32;
        app.down_cap = 10;
        let read = drive(&mut app, false).unwrap();
        assert_eq!(app.offset, 0);
        assert!(read.complete);
        assert_eq!(read.text, transcript(&app));
    }

    #[test]
    fn a_scrollbar_left_behind_after_scrolling_still_proves_restoration() {
        let mut app = App::new(60, 3);
        app.lingering_scrollbar = true;
        let read = drive(&mut app, false).unwrap();
        assert!(read.complete);
        assert!(read.text.starts_with(&transcript(&app)));
    }

    #[test]
    fn scrolls_are_batched_once_the_tick_size_is_known() {
        let mut app = App::new(400, 1);
        app.rows = 32;
        let mut now = Instant::now();
        let mut reader = AltScreenHistoryReader::new(app.frame(), now).unwrap();
        let mut largest = 0;
        loop {
            now += Duration::from_millis(100);
            match reader.observe(app.frame(), now) {
                HistoryReadStep::ScrollUp(ticks) => {
                    largest = largest.max(ticks);
                    app.wheel(true, ticks);
                }
                HistoryReadStep::ScrollDown(ticks) => app.wheel(false, ticks),
                HistoryReadStep::Finished(result) => {
                    assert!(result.unwrap().complete);
                    break;
                }
                HistoryReadStep::Wait => {}
            }
        }
        assert!(largest > 1, "every scroll was a single tick");
        assert_eq!(app.offset, 0);
    }

    #[test]
    fn a_traversal_limit_returns_the_newest_rows_as_incomplete() {
        let mut app = App::new(5000, 1);
        let read = drive(&mut app, false).unwrap();
        assert_eq!(app.offset, 0);
        assert!(!read.complete);
        let text = read.text;
        assert!(transcript(&app).ends_with(&text), "{text}");
        assert!(text.lines().count() > app.body(), "{text}");
    }

    #[test]
    fn manually_scrolled_view_is_undone_and_refused() {
        let mut app = App::new(60, 3);
        app.offset = 6;
        assert_eq!(drive(&mut app, false), Err(HistoryReadError::NotAtBottom));
        assert_eq!(app.offset, 6);
    }

    #[test]
    fn interruption_restores_before_releasing_the_lease() {
        let mut app = App::new(60, 3);
        assert_eq!(drive(&mut app, true), Err(HistoryReadError::Interrupted));
        assert_eq!(app.offset, 0);
    }

    #[test]
    fn a_transcript_that_fits_has_no_history() {
        let mut app = App::new(5, 3);
        assert_eq!(drive(&mut app, false), Err(HistoryReadError::Unavailable));
    }

    #[test]
    fn interruption_during_the_bottom_probe_preserves_the_starting_position() {
        for start in [6, 0] {
            let mut app = App::new(60, 3);
            app.offset = start;
            let mut now = Instant::now();
            let mut reader = AltScreenHistoryReader::new(app.frame(), now).unwrap();
            let mut interrupted = false;
            let mut result = None;
            for _ in 0..40 {
                now += Duration::from_millis(250);
                match reader.observe(app.frame(), now) {
                    HistoryReadStep::ScrollDown(ticks) => {
                        app.wheel(false, ticks);
                        if !interrupted {
                            reader.interrupt(now);
                            interrupted = true;
                        }
                    }
                    HistoryReadStep::ScrollUp(ticks) => app.wheel(true, ticks),
                    HistoryReadStep::Finished(end) => {
                        result = Some(end);
                        break;
                    }
                    HistoryReadStep::Wait => {}
                }
            }
            assert_eq!(result, Some(Err(HistoryReadError::Interrupted)));
            assert_eq!(app.offset, start);
        }
    }

    #[test]
    fn interruption_waits_for_an_in_flight_scroll_up() {
        let mut app = App::new(60, 3);
        let bottom = app.frame();
        let mut now = Instant::now();
        let mut reader = AltScreenHistoryReader::new(bottom.clone(), now).unwrap();
        let mut step = HistoryReadStep::Wait;
        for _ in 0..8 {
            now += Duration::from_millis(250);
            step = reader.observe(bottom.clone(), now);
            if matches!(step, HistoryReadStep::ScrollUp(_)) {
                break;
            }
        }
        assert_eq!(step, HistoryReadStep::ScrollUp(1));
        reader.interrupt(now);
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(bottom.clone(), now), HistoryReadStep::Wait);
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(bottom.clone(), now), HistoryReadStep::Wait);
        // The response finally lands; it must be observed and undone, not outrun.
        app.wheel(true, 1);
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(app.frame(), now), HistoryReadStep::Wait);
        now += Duration::from_millis(250);
        assert_eq!(
            reader.observe(app.frame(), now),
            HistoryReadStep::ScrollDown(1)
        );
        now += Duration::from_millis(250);
        assert_eq!(reader.observe(bottom.clone(), now), HistoryReadStep::Wait);
        now += Duration::from_millis(250);
        assert_eq!(
            reader.observe(bottom, now),
            HistoryReadStep::Finished(Err(HistoryReadError::Interrupted))
        );
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
    fn only_a_lone_glyph_at_the_right_edge_is_a_scrollbar() {
        let strip = |text: &str| without_scrollbar(&row(text), 20).text.to_string();
        assert_eq!(strip("日本語 🌷          ┃"), "日本語 🌷");
        assert_eq!(strip("text              █"), "text");
        assert_eq!(strip("                  │"), "");
        // Not at the edge, or part of the row's own drawing.
        assert_eq!(strip("  ┃ prompt"), "  ┃ prompt");
        assert_eq!(strip("text ┃"), "text ┃");
        assert_eq!(strip("╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀"), "╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀");
    }

    #[test]
    fn soft_wraps_join_rows_in_the_result() {
        let mut app = App::new(30, 3);
        let mut now = Instant::now();
        let wrap = |mut frame: HistoryReadFrame| {
            for row in &mut frame.rows {
                row.soft_wrapped = row.text.ends_with("line 004");
            }
            frame
        };
        let mut reader = AltScreenHistoryReader::new(wrap(app.frame()), now).unwrap();
        let read = loop {
            now += Duration::from_millis(100);
            match reader.observe(wrap(app.frame()), now) {
                HistoryReadStep::ScrollUp(ticks) => app.wheel(true, ticks),
                HistoryReadStep::ScrollDown(ticks) => app.wheel(false, ticks),
                HistoryReadStep::Finished(result) => break result.unwrap(),
                HistoryReadStep::Wait => {}
            }
        };
        assert!(read.text.contains("line 004line 005\n"), "{}", read.text);
    }

    #[test]
    fn repeated_rows_cannot_establish_an_alignment() {
        let repeated = vec![row("same"); 8];
        assert!(align(&repeated, &repeated, None).is_none());
        let blank = vec![row(""); 8];
        assert!(align(&blank, &blank, None).is_none());
        let previous = ["a", "b", "c", "d", "e", "f"].map(row);
        let changed = ["new", "different", "page", "x", "y", "z"].map(row);
        assert!(align(&previous, &changed, None).is_none());
    }

    #[test]
    fn pinned_rows_bound_the_transcript() {
        let previous = ["head", "c", "d", "e", "f", "g", "status", "foot"].map(row);
        let current = ["head", "a", "b", "c", "d", "e", "notice", "foot"].map(row);
        assert_eq!(
            align(&previous, &current, None),
            Some(Alignment {
                shift: 2,
                top: 1,
                bottom: 7,
                span: 5,
            })
        );
    }

    #[test]
    fn alignments_are_correct_or_refused_across_duplicate_row_corpora() {
        // Repeated and blank transcript rows between a pinned header and footer: an alignment
        // is either exact, boundaries included, or refused.
        let mut seed = 42u64;
        for _ in 0..4000 {
            let rows: Vec<_> = (0..16)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    match (seed >> 32) % 7 {
                        0 => row(""),
                        symbol => row(format!("{symbol}")),
                    }
                })
                .collect();
            for shift in 1..=4 {
                let page = |range: std::ops::Range<usize>| {
                    let mut page = vec![row("header")];
                    page.extend_from_slice(&rows[range]);
                    page.push(row("footer"));
                    page
                };
                let previous = page(4..12);
                let current = page(4 - shift..12 - shift);
                for known_top in [None, Some(1)] {
                    if let Some(alignment) = align(&previous, &current, known_top) {
                        assert_eq!(
                            (alignment.shift, alignment.top, alignment.bottom),
                            (shift, 1, previous.len() - 1),
                            "{previous:?} {current:?} {known_top:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn repeated_transcript_left_in_place_is_not_taken_for_a_footer() {
        let previous = ["head", "c", "d", "e", "f", "g", "h", "g", "h", "foot"].map(row);
        let current = ["head", "a", "b", "c", "d", "e", "f", "g", "h", "foot"].map(row);
        let alignment = align(&previous, &current, None).unwrap();
        assert_eq!((alignment.shift, alignment.top), (2, 1));
        assert_eq!(alignment.bottom, 9, "the trailing g, h are transcript");
    }

    #[test]
    fn a_blank_first_row_still_starts_the_transcript() {
        let previous = ["head", "", "c", "d", "e", "f", "g", "foot"].map(row);
        let current = ["head", "a", "b", "", "c", "d", "e", "foot"].map(row);
        let alignment = align(&previous, &current, None).unwrap();
        assert_eq!((alignment.shift, alignment.top), (2, 1));
    }

    #[test]
    fn a_row_both_moved_and_in_place_needs_a_known_top() {
        // The blank under the header is either a spacer or a transcript row: both fit.
        let previous = ["head", "", "c", "d", "e", "f", "g", "foot"].map(row);
        let current = ["head", "", "b", "", "c", "d", "e", "foot"].map(row);
        assert_eq!(align(&previous, &current, None), None);
        assert_eq!(align(&previous, &current, Some(2)).unwrap().top, 2);
        assert_eq!(align(&previous, &current, Some(1)).unwrap().top, 1);
    }

    #[test]
    fn resize_and_restoration_timeout_never_return_text() {
        let app = App::new(60, 3);
        let now = Instant::now();
        let mut reader = AltScreenHistoryReader::new(app.frame(), now).unwrap();
        let mut resized = app.frame();
        resized.viewport.cols = 80;
        assert_eq!(
            reader.observe(resized, now),
            HistoryReadStep::Finished(Err(HistoryReadError::RestoreUnproven))
        );
        let mut scrolled = app.clone();
        scrolled.wheel(true, 1);
        let mut reader = AltScreenHistoryReader::new(app.frame(), now).unwrap();
        assert_eq!(
            reader.observe(app.frame(), now + QUIET),
            HistoryReadStep::ScrollDown(1)
        );
        let moved_at = now + QUIET + RESPONSE;
        assert_eq!(
            reader.observe(app.frame(), moved_at),
            HistoryReadStep::ScrollUp(1)
        );
        reader.interrupt(moved_at);
        assert_eq!(
            // Restoration may take as long as the traversal before it, plus its own bound.
            reader.observe(scrolled.frame(), moved_at + RESTORATION + QUIET + RESPONSE),
            HistoryReadStep::Finished(Err(HistoryReadError::RestoreUnproven))
        );
    }

    #[test]
    fn synchronized_frames_do_not_settle_or_issue_scrolls() {
        let now = Instant::now();
        let mut pending = App::new(60, 3).frame();
        pending.synchronized = true;
        let mut reader = AltScreenHistoryReader::new(pending.clone(), now).unwrap();
        assert_eq!(
            reader.observe(pending, now + Duration::from_secs(1)),
            HistoryReadStep::Wait
        );
    }
}
