//! [`TerminalEngine`] over libghostty-vt.
//!
//! What libghostty-vt does not do the way the engine interface describes is made up for here:
//!
//! - **Semantic sequences.** Its callbacks parse OSC 133 and OSC 7 and drop the raw parameters the
//!   observer interprets (private `*_exe` keys, `cmdline_url`, OSC 9;9, the OSC 22 stack syntax),
//!   and it answers XTVERSION itself. So the shared [`semantic`](super::semantic) parser reads the
//!   raw bytes ahead of the grid, exactly as for Alacritty, and libghostty's own XTVERSION reply
//!   is dropped. The parser also reads the child's `DECSCUSR` requests, which libghostty cannot
//!   tell apart from its defaults.
//! - **Scrollback.** libghostty prunes whole pages, so it keeps somewhat more or fewer lines than
//!   asked. It is asked for [`ghostty_history_limit`] lines and only the newest `scrollback` of
//!   them are exposed; the rest are hidden below absolute line `0`. Evictions out of the exposed
//!   window are counted with a tracked grid reference on its oldest line, so they are exact.
//! - **Viewport.** libghostty's viewport stays on the live screen; the display offset is kept
//!   here, as Alacritty keeps it, and reads go through absolute lines.
//! - **The primary screen.** libghostty reads only the active screen. Reading the primary screen
//!   under the alternate one switches with `DECRST 47` and back, which clears the alternate
//!   screen, so it is repainted from a replay as Alacritty's repair does.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use alacritty_terminal::vte::Parser as SemanticVteParser;

use super::super::super::damage::{DamageAccumulator, TerminalDamage};
use super::super::super::ghostty::{
    ClipboardLocation, ClipboardWriteReply, ClipboardWriteResult, Mode, Point, RenderState, Rgb,
    Screen, SizeReport, StyleColor, Terminal, TrackedRef, Underline,
};
use super::super::{
    TerminalCellSize, TerminalClipboardEvent, TerminalClipboardTarget, TerminalColorPalette,
};
use super::semantic::{self, CursorRequests};
use super::{
    AnchorKind, CellAttrs, CursorAnchor, CursorTemplate, EngineAnchor, EngineConfig,
    EngineCursorShape, EngineCursorStyle, EngineObserver, GridPoint, GridStep, ModeFlags,
    NamedColor, PromptAnchor, ResizeOutcome, ResizeRequest, TerminalEngine, TrackedPoint,
};

mod read;

use read::{GhosttyLink, GhosttyRow, ReadScratch};

/// The most lines one piece of a write may push into history. Bounds how far one piece can
/// prune past the hidden history margin, which the eviction count relies on.
const LINE_BUDGET: usize = 1024;

/// Rows of one libghostty-vt page at `cols` columns, rounded up.
///
/// Pages have a fixed byte size, so they hold fewer rows the wider the screen. Measured on
/// libghostty-vt 1.0.1 by the retained-history floor: 2211 rows at 20 columns, 573 at 80, 383 at
/// 120, 231 at 200, 115 at 400, i.e. about 46,000 / columns. History is pruned a page at a time.
fn page_rows(cols: u16) -> usize {
    48_000 / usize::from(cols.max(1)) + 16
}

/// How many history lines to ask libghostty for, so that `scrollback` of them are always
/// exposed and the oldest exposed line survives any one piece of a write.
///
/// After pruning a page, libghostty keeps at least the limit minus one page; a piece then adds at
/// most [`LINE_BUDGET`] lines and can prune at most one more page. Two pages plus the budget above
/// `scrollback` covers both.
fn ghostty_history_limit(scrollback: usize, cols: u16) -> usize {
    scrollback
        .saturating_add(2 * page_rows(cols))
        .saturating_add(LINE_BUDGET)
}

/// `ED 3`: erase the scrollback.
const CLEAR_HISTORY: &[u8] = b"\x1b[3J";

/// Sequences that leave the alternate screen.
const LEAVE_ALTERNATE: [&[u8]; 3] = [b"\x1b[?1049l", b"\x1b[?1047l", b"\x1b[?47l"];

/// How many bytes at the end of `bytes` could still become [`CLEAR_HISTORY`] or one of
/// [`LEAVE_ALTERNATE`] once more output arrives: the longest suffix, starting at an escape, that
/// is a proper prefix of one of them.
fn special_sequence_prefix(bytes: &[u8]) -> usize {
    let longest = LEAVE_ALTERNATE
        .iter()
        .chain([&CLEAR_HISTORY])
        .map(|sequence| sequence.len())
        .max()
        .unwrap_or(0);
    let from = bytes.len().saturating_sub(longest - 1);
    (from..bytes.len())
        .find(|&start| {
            let tail = &bytes[start..];
            tail[0] == 0x1b
                && LEAVE_ALTERNATE
                    .iter()
                    .chain([&CLEAR_HISTORY])
                    .any(|sequence| sequence.len() > tail.len() && sequence.starts_with(tail))
        })
        .map_or(0, |start| bytes.len() - start)
}

/// Only advertise protocols the engine contract can deliver. Resize has no observer, and
/// hosts drain replies after child output, so an idle child waiting for a mode 2048 resize
/// report would stall. Keep it on SIGWINCH, like Alacritty, until resize replies have a path.
fn compatible_response(reply: &[u8]) -> Option<&[u8]> {
    if reply.starts_with(b"\x1b[?2048;") {
        return Some(b"\x1b[?2048;0$y");
    }
    if reply.starts_with(b"\x1b[48;") {
        return None;
    }
    // The shared screen answers XTVERSION and owns the optional OSC 72 protocol.
    (!reply.starts_with(b"\x1bP>|") && !reply.starts_with(b"\x1b]72;")).then_some(reply)
}

/// Where to split a write so each piece can be measured on its own: `ED 3` alone, a sequence
/// leaving the alternate screen alone (so leaving and reopening it within one write is still
/// seen), and a cut before a piece could push more than [`LINE_BUDGET`] lines into history.
///
/// Lines are over-counted: every line feed, vertical tab, form feed and escape (for `IND`/`NEL`),
/// and one per `cols` bytes of text for autowraps.
fn write_boundaries(bytes: &[u8], cols: u16) -> Vec<usize> {
    let cols = usize::from(cols.max(1));
    let mut ends = Vec::new();
    let mut start = 0;
    let mut lines = 0;
    let cut = |ends: &mut Vec<usize>, start: &mut usize, lines: &mut usize, at: usize| {
        if at > *start {
            ends.push(at);
            *start = at;
            *lines = 0;
        }
    };
    let mut feeds = memchr::memchr3_iter(b'\n', 0x0b, 0x0c, bytes).peekable();
    let mut escapes = memchr::memchr_iter(0x1b, bytes).peekable();
    loop {
        let next = match (feeds.peek(), escapes.peek()) {
            (None, None) => break,
            (Some(&feed), Some(&escape)) => feed.min(escape),
            (Some(&feed), None) => feed,
            (None, Some(&escape)) => escape,
        };
        if feeds.peek() == Some(&next) {
            feeds.next();
        } else {
            escapes.next();
        }
        // A long run of text wraps as it goes.
        while lines + (next - start) / cols >= LINE_BUDGET {
            let at = start + (LINE_BUDGET - lines).max(1) * cols;
            cut(&mut ends, &mut start, &mut lines, at.min(next));
        }
        lines += 1;
        if bytes[next] != 0x1b {
            continue;
        }
        let rest = &bytes[next..];
        if rest.starts_with(CLEAR_HISTORY) {
            cut(&mut ends, &mut start, &mut lines, next);
            cut(
                &mut ends,
                &mut start,
                &mut lines,
                next + CLEAR_HISTORY.len(),
            );
        } else if let Some(sequence) = LEAVE_ALTERNATE.iter().find(|s| rest.starts_with(s)) {
            // Alone in its piece, so `write` can see it and drop a 1049 reset on the primary
            // screen.
            cut(&mut ends, &mut start, &mut lines, next);
            cut(&mut ends, &mut start, &mut lines, next + sequence.len());
        }
    }
    while lines + (bytes.len() - start) / cols >= LINE_BUDGET {
        let at = start + (LINE_BUDGET - lines).max(1) * cols;
        cut(&mut ends, &mut start, &mut lines, at.min(bytes.len()));
    }
    cut(&mut ends, &mut start, &mut lines, bytes.len());
    ends
}

/// Runs of non-ASCII text longer than this many bytes are handed to libghostty in pieces of
/// [`TEXT_RUN_PIECE`] bytes; see [`write_text_runs_bounded`]. Long enough that an 80-column
/// row of emoji or CJK is written whole.
const LONG_TEXT_RUN: usize = 192;

/// See [`write_text_runs_bounded`]. Measured flat between 16 and 32 bytes, worse at 8 and 64.
const TEXT_RUN_PIECE: usize = 32;

/// Write `bytes`, cutting long runs of non-ASCII text into short writes.
///
/// libghostty-vt 1.0.1 batches a run of printable codepoints, but when a cell it overwrites is
/// not a plain narrow cell (a wide character or its spacer, as when a full-screen program
/// redraws CJK or emoji in place) it prints that one codepoint the slow way and then scans the
/// rest of the run again, so overwriting a run costs time quadratic in its length: 41 ns per
/// CJK cell at 200 columns, 135 ns at 1000, against 17 ns in 32-byte writes. A write boundary
/// ends the run and so bounds the rescan. ASCII and runs up to [`LONG_TEXT_RUN`] bytes are
/// written whole: shorter runs gain nothing and every extra write costs a little.
fn write_text_runs_bounded(terminal: &mut Terminal, bytes: &[u8]) {
    if bytes.len() <= LONG_TEXT_RUN {
        terminal.write(bytes);
        return;
    }
    let mut written = 0;
    let mut start = 0;
    // Escapes and line breaks end a run of text. Look a window at a time for the last one: in
    // ordinary output there is one in every window, so this costs one search per window.
    while bytes.len() - start > LONG_TEXT_RUN {
        let window = &bytes[start..=start + LONG_TEXT_RUN];
        if let Some(last) = memchr::memrchr3(0x1b, b'\r', b'\n', window) {
            start += last + 1;
            continue;
        }
        let end = memchr::memchr3(0x1b, b'\r', b'\n', &bytes[start + LONG_TEXT_RUN..])
            .map_or(bytes.len(), |offset| start + LONG_TEXT_RUN + offset);
        if !bytes[start..end].is_ascii() {
            terminal.write(&bytes[written..start]);
            let mut piece = start;
            while piece < end {
                let mut cut = (piece + TEXT_RUN_PIECE).min(end);
                // Keep UTF-8 sequences whole.
                while cut < end && bytes[cut] & 0xC0 == 0x80 {
                    cut += 1;
                }
                terminal.write(&bytes[piece..cut]);
                piece = cut;
            }
            written = end;
        }
        start = end;
    }
    terminal.write(&bytes[written..]);
}

/// What libghostty's callbacks report, collected until the write that caused them returns.
#[derive(Default)]
struct Effects {
    responses: RefCell<Vec<Vec<u8>>>,
    clipboard: RefCell<Vec<TerminalClipboardEvent>>,
    bells: Cell<u64>,
    reset: Cell<bool>,
    /// OSC 0/2 title. Kept here because libghostty clears its own on RIS, and Alacritty does not.
    title: RefCell<Option<String>>,
    geometry: Cell<(u16, u16, TerminalCellSize)>,
}

/// libghostty-vt, with a hidden history margin and a viewport kept here.
pub(in super::super) struct GhosttyEngine {
    terminal: Terminal,
    /// Brought up to date when damage is taken; libghostty keeps the dirty rows until then.
    render: RenderState,
    damage: DamageAccumulator,
    effects: Rc<Effects>,
    semantic_parser: SemanticVteParser,
    cursor_requests: CursorRequests,
    default_cursor_style: EngineCursorStyle,
    scratch: RefCell<ReadScratch>,
    /// History lines exposed to callers.
    scrollback: usize,
    /// libghostty's compression-activity token when compression last finished: nothing to do
    /// until it changes.
    compressed_at: Option<u64>,
    /// How far the viewport is scrolled into the exposed history.
    display_offset: usize,
    /// The oldest exposed line of the primary screen, and how many history lines were exposed
    /// when it was taken: what the next write's evictions are measured against.
    window_top: Option<TrackedRef>,
    window_history: usize,
    /// The end of the last write that may still become a sequence [`Self::write`] treats
    /// specially (see [`special_sequence_prefix`]); written ahead of the next write. Recognizing
    /// it here instead of in libghostty's parser keeps the treatment independent of how the PTY
    /// reads split the output. It is part of an unfinished escape sequence, so holding it back
    /// changes no cell, cursor or reply.
    held: Vec<u8>,
}

impl GhosttyEngine {
    fn new_terminal(config: &EngineConfig, effects: &Rc<Effects>) -> Terminal {
        let mut terminal = Terminal::new(config.cols, config.rows)
            .unwrap_or_else(|error| panic!("libghostty-vt terminal: {error}"));
        let limit = ghostty_history_limit(config.scrollback, config.cols);
        let result = (|| {
            terminal.set_scrollback_limits(super::super::super::ghostty::ScrollbackLimits {
                max_lines: Some(limit),
                max_bytes: None,
            })?;
            let sink = Rc::clone(effects);
            terminal.on_write_pty(Some(Box::new(move |reply: &[u8]| {
                if let Some(reply) = compatible_response(reply) {
                    sink.responses.borrow_mut().push(reply.to_vec());
                }
            })))?;
            let sink = Rc::clone(effects);
            terminal.on_bell(Some(Box::new(move || {
                sink.bells.set(sink.bells.get().saturating_add(1));
            })))?;
            let sink = Rc::clone(effects);
            terminal.on_reset(Some(Box::new(move || sink.reset.set(true))))?;
            let sink = Rc::clone(effects);
            terminal.on_title_changed(Some(Box::new(move |title: &[u8]| {
                *sink.title.borrow_mut() = Some(String::from_utf8_lossy(title).into_owned());
            })))?;
            let sink = Rc::clone(effects);
            terminal.on_clipboard_write(Some(Box::new(move |write| {
                let target = match write.location {
                    ClipboardLocation::Standard => TerminalClipboardTarget::Clipboard,
                    ClipboardLocation::Selection | ClipboardLocation::Primary => {
                        TerminalClipboardTarget::Selection
                    }
                };
                for content in &write.contents {
                    if content.mime.starts_with(b"text/plain")
                        && let Ok(text) = std::str::from_utf8(content.data)
                    {
                        sink.clipboard.borrow_mut().push(TerminalClipboardEvent {
                            target,
                            text: text.to_owned(),
                        });
                    }
                }
                ClipboardWriteReply {
                    result: ClipboardWriteResult::Success,
                    remember: false,
                }
            })))?;
            let sink = Rc::clone(effects);
            terminal.on_size(Some(Box::new(move || {
                let (rows, columns, cell) = sink.geometry.get();
                Some(SizeReport {
                    rows,
                    columns,
                    cell_width: u32::from(cell.width),
                    cell_height: u32::from(cell.height),
                })
            })))?;
            Ok::<_, super::super::super::ghostty::GhosttyError>(())
        })();
        if let Err(error) = result {
            panic!("libghostty-vt configuration: {error}");
        }
        terminal
    }

    pub(in super::super) fn set_default_grapheme_clustering(&mut self, enabled: bool) {
        self.terminal
            .set_default_mode(Mode::GRAPHEME_CLUSTER, enabled)
            .unwrap_or_else(|error| panic!("libghostty-vt grapheme default: {error}"));
    }

    fn set_history_limit(&mut self, lines: usize) {
        let _ =
            self.terminal
                .set_scrollback_limits(super::super::super::ghostty::ScrollbackLimits {
                    max_lines: Some(lines),
                    max_bytes: None,
                });
    }

    #[cfg(test)]
    pub(in super::super) fn scrollback_limit(&self) -> usize {
        self.scrollback
    }

    fn alternate(&self) -> bool {
        self.terminal.active_screen() == Screen::Alternate
    }

    /// History lines libghostty keeps beyond the exposed window.
    fn hidden(&self) -> usize {
        let history = self.terminal.scrollback_rows();
        history - history.min(self.scrollback)
    }

    /// Fold libghostty's dirty rows into the damage since the last take.
    fn collect_damage(&mut self) {
        let damage =
            read::take_damage(&mut self.render, &mut self.terminal, self.scratch.get_mut());
        match damage {
            TerminalDamage::None => {}
            TerminalDamage::Full => self.damage.mark_full(),
            TerminalDamage::Rows(rows) => {
                let viewport_rows = usize::from(self.terminal.rows());
                for row in rows {
                    self.damage.add_row(
                        usize::from(row.row),
                        usize::from(row.left),
                        usize::from(row.right),
                        viewport_rows,
                    );
                }
            }
        }
    }

    /// Write `bytes` to libghostty in bounded pieces, measuring each.
    fn write(&mut self, bytes: &[u8]) -> GridStep {
        let joined;
        let bytes = if self.held.is_empty() {
            bytes
        } else {
            let mut held = std::mem::take(&mut self.held);
            held.extend_from_slice(bytes);
            joined = held;
            &joined[..]
        };
        let (bytes, held) = bytes.split_at(bytes.len() - special_sequence_prefix(bytes));
        self.held = held.to_vec();
        self.write_whole(bytes)
    }

    /// [`Self::write`] for bytes that hold no unfinished special sequence at the end.
    fn write_whole(&mut self, bytes: &[u8]) -> GridStep {
        self.scratch.get_mut().invalidate();
        let mut total = GridStep::default();
        let mut start = 0;
        for end in write_boundaries(bytes, self.terminal.cols()) {
            let piece = &bytes[start..end];
            start = end;
            let alternate = self.alternate();
            if piece == LEAVE_ALTERNATE[0] && !alternate {
                // On the primary screen libghostty, like xterm, still restores the cursor saved
                // by `DECSC`/`?1049h`, (0, 0) if none was. Alacritty ignores it, and shells send
                // it after every command to recover from a crashed full-screen program, so each
                // new prompt would jump to the top line.
                continue;
            }
            self.effects.reset.set(false);
            write_text_runs_bounded(&mut self.terminal, piece);
            let reset = self.effects.reset.take();
            if reset {
                self.window_top = None;
                self.window_history = 0;
                self.display_offset = 0;
            }
            let evicted = if piece == CLEAR_HISTORY && !alternate {
                // `ED 3` drops the history without invalidating references into it: everything
                // exposed above the live screen went, as Alacritty's ledger counts it.
                let evicted = self.window_history;
                self.window_top = None;
                self.window_history = 0;
                self.display_offset = 0;
                evicted + self.settle_window(false)
            } else {
                self.settle_window(true)
            };
            self.sync_cursor_blink();
            total.merge(GridStep {
                evicted,
                reset,
                left_alt_screen: alternate && !self.alternate(),
                grid_mutated: true,
            });
        }
        total
    }

    /// While the child has not asked for a blink setting, mode 12 follows the default in force,
    /// so `DECRQM 12` answers with it as Alacritty does.
    fn sync_cursor_blink(&mut self) {
        if !self.cursor_requests.blinking {
            let blinking = self.default_cursor_style.blinking;
            if self.terminal.mode(Mode::CURSOR_BLINKING) != Some(blinking) {
                let _ = self.terminal.set_mode(Mode::CURSOR_BLINKING, blinking);
            }
        }
    }

    /// Count the exposed lines that went out of the window since it was last measured, and
    /// measure it again. A no-op while the alternate screen is active: only the primary screen
    /// has history, and it does not move meanwhile.
    fn settle_window(&mut self, follow_viewport: bool) -> usize {
        if self.alternate() {
            return 0;
        }
        let history = self.terminal.scrollback_rows();
        let exposed = history.min(self.scrollback);
        let hidden = history - exposed;
        let evicted = match &self.window_top {
            Some(top) if top.has_value() => match top.point(Point::Screen { x: 0, y: 0 }) {
                Some(Point::Screen { y, .. }) => hidden.saturating_sub(y as usize),
                _ => 0,
            },
            // The oldest exposed line itself is gone: everything above the live screen went,
            // as `ED 3` does.
            Some(_) => self.window_history + hidden,
            None => 0,
        };
        if follow_viewport && self.display_offset > 0 {
            // Like Alacritty, a scrolled viewport stays on the lines it shows.
            let pushed = (exposed + evicted).saturating_sub(self.window_history);
            self.display_offset = (self.display_offset + pushed).min(exposed);
        }
        // Keep the reference while it still marks the window's top; making one costs more.
        let still_top = self.window_top.as_ref().is_some_and(|top| {
            top.has_value()
                && matches!(
                    top.point(Point::Screen { x: 0, y: 0 }),
                    Some(Point::Screen { x: 0, y }) if y as usize == hidden
                )
        });
        if !still_top {
            self.window_top = u32::try_from(hidden)
                .ok()
                .and_then(|y| self.terminal.track(Point::Screen { x: 0, y }).ok());
        }
        self.window_history = exposed;
        evicted
    }

    fn deliver<O: EngineObserver>(&mut self, step: GridStep, observer: &mut O) -> GridStep {
        let responses = std::mem::take(&mut *self.effects.responses.borrow_mut());
        for response in responses {
            observer.respond(response);
        }
        observer.grid_step(step);
        step
    }

    fn cursor_anchor(&self) -> CursorAnchor {
        let (x, y) = self.terminal.cursor_position();
        CursorAnchor {
            absolute_line: self.terminal.scrollback_rows() - self.hidden() + usize::from(y),
            column: usize::from(x),
            alt_screen: self.alternate(),
        }
    }

    /// Track the cell at absolute line `line`, column `column` of the active screen.
    fn track_absolute(&mut self, line: usize, column: u16) -> Option<TrackedRef> {
        let y = u32::try_from(self.hidden() + line).ok()?;
        let column = column.min(self.terminal.cols().saturating_sub(1));
        self.terminal.track(Point::Screen { x: column, y }).ok()
    }

    /// Where a tracked cell is now, as an absolute line and column.
    fn tracked_absolute(&self, tracked: &TrackedRef) -> Option<(usize, u16)> {
        if !tracked.has_value() {
            return None;
        }
        let Point::Screen { x, y } = tracked.point(Point::Screen { x: 0, y: 0 })? else {
            return None;
        };
        (y as usize)
            .checked_sub(self.hidden())
            .map(|line| (line, x))
    }

    /// Erase the active prompt before a reflow, so the shell's redraw after `SIGWINCH` is not
    /// added on top of the old prompt. [`Self::resize_with_prompt`] then reflows with the cursor
    /// parked on the prompt's start, as Alacritty's prompt resize does.
    ///
    /// Only the part on the live screen can be erased: libghostty cannot write into history, so a
    /// prompt that starts above the screen keeps its first rows.
    ///
    /// The erase and every cursor move here are direct grid calls, never VT through the child's
    /// parser, so a sequence the child left unfinished completes as written after the resize.
    fn prepare_prompt_resize(&mut self, mark: PromptAnchor) -> Option<PromptResize> {
        if self.alternate() {
            return None;
        }
        let history = self.history_size();
        let rows = usize::from(self.terminal.rows());
        if mark.absolute_line >= history + rows {
            return None;
        }
        let cols = usize::from(self.terminal.cols());
        let column = mark.column.min(cols);
        let anchor = self.track_absolute(mark.absolute_line, column.min(cols - 1) as u16)?;
        let (x, y) = self.terminal.cursor_position();
        let anchor_row = mark.absolute_line as isize - history as isize;
        let cursor_line_offset = (y as isize - anchor_row).max(0) as usize;
        // As Alacritty erases it: the anchor's row from its column, and every row below.
        let erase_from = match usize::try_from(anchor_row) {
            Ok(row) if column < cols => Some((column, row)),
            Ok(row) => (row + 1 < rows).then_some((0, row + 1)),
            Err(_) => Some((0, 0)),
        };
        if let Some((column, row)) = erase_from {
            let _ = self.terminal.set_cursor_position(column as u16, row as u16);
            let _ = self.terminal.erase_below();
        }
        // Back to the shell's cursor: the height change scrolls against it.
        let _ = self.terminal.set_cursor_position(x, y);
        Some(PromptResize {
            anchor,
            cursor_line_offset,
            cursor_column: usize::from(x),
        })
    }

    /// Resize with an active prompt, in Alacritty's order: the height first against the shell's
    /// real cursor; then, with the cursor parked on the prompt's start (pulled out of history with
    /// temporary extra rows if needed), the width; then the cursor goes back to the same row
    /// offset from the prompt.
    fn resize_with_prompt(&mut self, resize: &PromptResize, rows: u16, cols: u16) {
        let (_, _, cell) = self.effects.geometry.get();
        let (width, height) = (u32::from(cell.width), u32::from(cell.height));
        let old_cols = self.terminal.cols();
        let _ = self.terminal.resize(old_cols, rows, width, height);
        let extra = self
            .tracked_absolute(&resize.anchor)
            .map_or(0, |(line, _)| self.history_size().saturating_sub(line));
        let reflow_rows = rows.saturating_add(u16::try_from(extra).unwrap_or(u16::MAX));
        if reflow_rows != rows {
            let _ = self.terminal.resize(old_cols, reflow_rows, width, height);
        }
        if let Some((line, column)) = self.tracked_absolute(&resize.anchor)
            && let Some(row) = line.checked_sub(self.history_size())
        {
            let _ = self.terminal.set_cursor_position(column, row as u16);
        }
        let _ = self.terminal.resize(cols, reflow_rows, width, height);
        if let Some((line, _)) = self.tracked_absolute(&resize.anchor) {
            let anchor_row = line as isize - self.history_size() as isize;
            let row = (anchor_row + resize.cursor_line_offset as isize)
                .clamp(0, reflow_rows as isize - 1);
            let col = resize.cursor_column.min(usize::from(cols) - 1);
            let _ = self.terminal.set_cursor_position(col as u16, row as u16);
        }
        if reflow_rows != rows {
            let _ = self.terminal.resize(cols, rows, width, height);
        }
    }

    /// Where the prompt's start landed.
    fn finish_prompt_resize(&self, resize: &PromptResize) -> Option<PromptAnchor> {
        let (line, column) = self.tracked_absolute(&resize.anchor)?;
        Some(PromptAnchor {
            absolute_line: line,
            column: usize::from(column),
        })
    }
}

/// An active prompt carried through a resize.
struct PromptResize {
    anchor: TrackedRef,
    cursor_line_offset: usize,
    cursor_column: usize,
}

impl TerminalEngine for GhosttyEngine {
    type Link = GhosttyLink;
    type Row<'a> = GhosttyRow;

    fn new(config: EngineConfig) -> Self {
        let effects = Rc::new(Effects::default());
        effects
            .geometry
            .set((config.rows, config.cols, TerminalCellSize::default()));
        let mut terminal = Self::new_terminal(&config, &effects);
        let mut render =
            RenderState::new().unwrap_or_else(|error| panic!("libghostty-vt render: {error}"));
        let _ = render.update(&mut terminal);
        render.clean();
        let mut engine = Self {
            terminal,
            render,
            damage: DamageAccumulator::default(),
            effects,
            semantic_parser: SemanticVteParser::new(),
            cursor_requests: CursorRequests::default(),
            default_cursor_style: config.default_cursor_style,
            scratch: RefCell::new(ReadScratch::default()),
            scrollback: config.scrollback,
            compressed_at: None,
            display_offset: 0,
            window_top: None,
            window_history: 0,
            held: Vec::new(),
        };
        engine.set_palette(&TerminalColorPalette::default());
        engine.sync_cursor_blink();
        engine.settle_window(false);
        engine
    }

    fn reset(&mut self) {
        self.scratch.get_mut().invalidate();
        self.terminal.reset();
        self.effects.responses.borrow_mut().clear();
        self.effects.clipboard.borrow_mut().clear();
        self.effects.bells.set(0);
        self.effects.reset.set(false);
        *self.effects.title.borrow_mut() = None;
        self.semantic_parser = SemanticVteParser::new();
        self.cursor_requests = CursorRequests::default();
        self.held.clear();
        self.display_offset = 0;
        self.window_top = None;
        self.window_history = 0;
        self.settle_window(false);
        self.sync_cursor_blink();
        self.collect_damage();
    }

    fn set_default_cursor_style(&mut self, style: EngineCursorStyle) -> bool {
        if self.default_cursor_style == style {
            return false;
        }
        self.default_cursor_style = style;
        self.sync_cursor_blink();
        true
    }

    fn set_palette(&mut self, palette: &TerminalColorPalette) {
        use crate::style::Color as UiColor;
        let standard = |index: usize| UiColor::Indexed(index as u8).to_rgb().unwrap_or((0, 0, 0));
        let rgb = |(r, g, b): (u8, u8, u8)| Rgb { r, g, b };
        let colors: [Rgb; 256] = std::array::from_fn(|index| {
            rgb(if index < 16 {
                palette.ansi[index]
                    .to_rgb()
                    .unwrap_or_else(|| standard(index))
            } else {
                standard(index)
            })
        });
        let foreground = palette
            .foreground
            .and_then(UiColor::to_rgb)
            .unwrap_or((255, 255, 255));
        let background = palette
            .background
            .and_then(UiColor::to_rgb)
            .unwrap_or((0, 0, 0));
        let _ = self.terminal.set_default_palette(Some(&colors));
        let _ =
            self.terminal
                .set_default_colors(Some(rgb(foreground)), Some(rgb(background)), None);
    }

    fn set_report_geometry(&mut self, rows: u16, cols: u16, cell: TerminalCellSize) {
        self.effects.geometry.set((rows, cols, cell));
    }

    fn scan_ahead<O: EngineObserver>(
        &mut self,
        bytes: &[u8],
        observer: &mut O,
    ) -> Vec<(usize, O::Anchor)> {
        let mut anchors = Vec::new();
        let mut processed = 0;
        while processed < bytes.len() {
            let consumed = semantic::advance_until_anchor(
                &mut self.semantic_parser,
                observer,
                Some(&mut self.cursor_requests),
                &bytes[processed..],
            );
            if consumed == 0 {
                break;
            }
            processed += consumed;
            if let Some(anchor) = observer.take_anchor() {
                anchors.push((processed, anchor));
            }
        }
        anchors
    }

    /// libghostty applies a synchronized update as it arrives, so the cursor is always current.
    fn anchor<O: EngineObserver>(&mut self, anchor: O::Anchor, observer: &mut O) -> GridStep {
        let cursor = match anchor.kind() {
            AnchorKind::Order => None,
            AnchorKind::Cursor => Some(self.cursor_anchor()),
        };
        observer.anchor(anchor, cursor);
        GridStep::default()
    }

    fn feed<O: EngineObserver>(&mut self, bytes: &[u8], observer: &mut O) -> GridStep {
        if bytes.is_empty() {
            return GridStep::default();
        }
        let step = self.write(bytes);
        self.deliver(step, observer)
    }

    /// Mode 2026 is a hint here: libghostty does not hold the update back.
    fn sync_pending(&self) -> bool {
        self.terminal.mode(Mode::SYNCHRONIZED_OUTPUT) == Some(true)
    }

    fn commit_sync<O: EngineObserver>(&mut self, _observer: &mut O) -> GridStep {
        GridStep::default()
    }

    fn resume_sync<O: EngineObserver>(&mut self, _observer: &mut O) -> GridStep {
        GridStep::default()
    }

    fn take_damage(&mut self) -> TerminalDamage {
        self.collect_damage();
        let damage = self.damage.take();
        // libghostty's dirty rows are rows of the live screen; a scrolled view shows others.
        if self.display_offset > 0 && damage != TerminalDamage::None {
            return TerminalDamage::Full;
        }
        damage
    }

    fn resize(&mut self, request: ResizeRequest<'_>) -> ResizeOutcome {
        self.scratch.get_mut().invalidate();
        let ResizeRequest {
            rows,
            cols,
            prompt,
            track,
        } = request;
        // Pages hold fewer rows when wider: grow the margin before the reflow, settle it after.
        let old_limit = ghostty_history_limit(self.scrollback, self.terminal.cols());
        let new_limit = ghostty_history_limit(self.scrollback, cols);
        self.set_history_limit(old_limit.max(new_limit));
        let reflowed = cols != self.terminal.cols();
        let prompt = prompt.and_then(|mark| self.prepare_prompt_resize(mark));
        let tracked: Option<Vec<Option<TrackedRef>>> = track.filter(|_| reflowed).map(|points| {
            let alternate = self.alternate();
            points
                .iter()
                .map(|point: &TrackedPoint| {
                    (point.alt_screen == alternate)
                        .then(|| self.track_absolute(point.absolute_line, point.column))
                        .flatten()
                })
                .collect()
        });
        // The line at the top of a scrolled viewport stays there through the reflow.
        let viewport_top = (self.display_offset > 0)
            .then(|| {
                let line = self.history_size() - self.display_offset();
                self.track_absolute(line, 0)
            })
            .flatten();
        match &prompt {
            Some(resize) => self.resize_with_prompt(resize, rows, cols),
            None => {
                // The pinned sys build patches Screen/PageList resize to crop the alternate
                // screen below the cursor first, matching Alacritty. Keep this a native resize:
                // injecting CUP/ED would disturb pending wrap, saved cursors, DECOM, and any
                // unfinished VT sequence. See ghostty-vt-sys/patches/libghostty-vt.patch.
                let (_, _, cell) = self.effects.geometry.get();
                let _ =
                    self.terminal
                        .resize(cols, rows, u32::from(cell.width), u32::from(cell.height));
            }
        }
        self.set_history_limit(new_limit);
        let evicted = self.settle_window(false);
        self.display_offset = match viewport_top {
            Some(top) => self
                .tracked_absolute(&top)
                .map_or(0, |(line, _)| self.history_size().saturating_sub(line)),
            None => 0,
        };
        // Pre-eviction coordinates, as the screen shifts the anchor by `evicted` itself.
        let prompt = prompt
            .and_then(|resize| self.finish_prompt_resize(&resize))
            .map(|anchor| PromptAnchor {
                absolute_line: anchor.absolute_line + evicted,
                ..anchor
            });
        let relocated = tracked.map(|refs| {
            refs.iter()
                .map(|tracked| tracked.as_ref().and_then(|t| self.tracked_absolute(t)))
                .collect()
        });
        self.collect_damage();
        ResizeOutcome {
            evicted,
            prompt,
            relocated,
        }
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.terminal.rows())
    }

    fn columns(&self) -> usize {
        usize::from(self.terminal.cols())
    }

    fn history_size(&self) -> usize {
        self.terminal.scrollback_rows().min(self.scrollback)
    }

    fn display_offset(&self) -> usize {
        self.display_offset.min(self.history_size())
    }

    fn scroll_display_to(&mut self, offset: usize) {
        self.display_offset = offset.min(self.history_size());
    }

    fn modes(&self) -> ModeFlags {
        let mut flags = ModeFlags::empty();
        let on = |mode: Mode| self.terminal.mode(mode) == Some(true);
        for (mode, flag) in [
            (Mode::CURSOR_VISIBLE, ModeFlags::SHOW_CURSOR),
            (Mode::CURSOR_KEYS, ModeFlags::APP_CURSOR),
            (Mode::KEYPAD_KEYS, ModeFlags::APP_KEYPAD),
            (Mode::MOUSE_EVENT_NORMAL, ModeFlags::MOUSE_REPORT_CLICK),
            (Mode::BRACKETED_PASTE, ModeFlags::BRACKETED_PASTE),
            (Mode::MOUSE_FORMAT_SGR, ModeFlags::SGR_MOUSE),
            (Mode::MOUSE_EVENT_ANY, ModeFlags::MOUSE_MOTION),
            (Mode::WRAPAROUND, ModeFlags::LINE_WRAP),
            (Mode::FOCUS_EVENT, ModeFlags::FOCUS_IN_OUT),
            (Mode::MOUSE_EVENT_BUTTON, ModeFlags::MOUSE_DRAG),
            (Mode::MOUSE_FORMAT_UTF8, ModeFlags::UTF8_MOUSE),
            (Mode::MOUSE_FORMAT_SGR_PIXELS, ModeFlags::SGR_PIXELS_MOUSE),
        ] {
            flags.set(flag, on(mode));
        }
        flags.set(ModeFlags::ALT_SCREEN, self.alternate());
        let kitty = self.terminal.kitty_keyboard_flags();
        for (bit, flag) in [
            (1, ModeFlags::DISAMBIGUATE_ESC_CODES),
            (2, ModeFlags::REPORT_EVENT_TYPES),
            (4, ModeFlags::REPORT_ALTERNATE_KEYS),
            (8, ModeFlags::REPORT_ALL_KEYS_AS_ESC),
            (16, ModeFlags::REPORT_ASSOCIATED_TEXT),
        ] {
            flags.set(flag, kitty & bit != 0);
        }
        flags
    }

    fn mode_fingerprint(&self) -> u64 {
        const TRACKED: [Mode; 21] = [
            Mode::INSERT,
            Mode::LINEFEED,
            Mode::CURSOR_KEYS,
            Mode::REVERSE_COLORS,
            Mode::ORIGIN,
            Mode::WRAPAROUND,
            Mode::MOUSE_EVENT_X10,
            Mode::CURSOR_BLINKING,
            Mode::CURSOR_VISIBLE,
            Mode::KEYPAD_KEYS,
            Mode::MOUSE_EVENT_NORMAL,
            Mode::MOUSE_EVENT_BUTTON,
            Mode::MOUSE_EVENT_ANY,
            Mode::FOCUS_EVENT,
            Mode::MOUSE_FORMAT_UTF8,
            Mode::MOUSE_FORMAT_SGR,
            Mode::MOUSE_ALTERNATE_SCROLL,
            Mode::MOUSE_FORMAT_URXVT,
            Mode::BRACKETED_PASTE,
            Mode::SYNCHRONIZED_OUTPUT,
            Mode::MOUSE_FORMAT_SGR_PIXELS,
        ];
        let mut bits = 0u64;
        for (index, mode) in TRACKED.iter().enumerate() {
            if self.terminal.mode(*mode) == Some(true) {
                bits |= 1 << index;
            }
        }
        bits |= u64::from(self.terminal.kitty_keyboard_flags()) << 32;
        bits |= u64::from(self.alternate()) << 40;
        bits
    }

    fn cursor(&self) -> GridPoint {
        let (x, y) = self.terminal.cursor_position();
        GridPoint {
            line: i32::from(y),
            column: usize::from(x),
        }
    }

    fn display_cursor(&self) -> GridPoint {
        let mut point = self.cursor();
        let (x, y) = self.terminal.cursor_position();
        if x > 0
            && self
                .terminal
                .cell_wide(Point::Active { x, y: u32::from(y) })
                == Some(super::super::super::ghostty::CellWide::SpacerTail)
        {
            point.column -= 1;
        }
        point
    }

    fn cursor_style(&self) -> EngineCursorStyle {
        // Read from the requests rather than the render state, which is only brought up to date
        // when damage is taken.
        let shape = match (
            self.cursor_requests.osc_shape,
            self.cursor_requests.decscusr,
        ) {
            (Some(0), _) => EngineCursorShape::Block,
            (Some(1), _) => EngineCursorShape::Beam,
            (Some(_), _) => EngineCursorShape::Underline,
            (None, Some(3 | 4)) => EngineCursorShape::Underline,
            (None, Some(5 | 6)) => EngineCursorShape::Beam,
            (None, _) => EngineCursorShape::Block,
        };
        EngineCursorStyle {
            shape: if self.cursor_requests.shape {
                shape
            } else {
                self.default_cursor_style.shape
            },
            blinking: if self.cursor_requests.blinking {
                self.terminal.mode(Mode::CURSOR_BLINKING) == Some(true)
            } else {
                self.default_cursor_style.blinking
            },
        }
    }

    fn cursor_color(&self) -> Option<(u8, u8, u8)> {
        self.terminal
            .cursor_color()
            .map(|rgb| (rgb.r, rgb.g, rgb.b))
    }

    fn cursor_template(&self) -> CursorTemplate<GhosttyLink> {
        let style = self.terminal.cursor_style();
        let mut attrs = CellAttrs::empty();
        for (on, attr) in [
            (style.bold, CellAttrs::BOLD),
            (style.italic, CellAttrs::ITALIC),
            (style.faint, CellAttrs::DIM),
            (style.inverse, CellAttrs::INVERSE),
            (style.invisible, CellAttrs::HIDDEN),
            (style.strikethrough, CellAttrs::STRIKEOUT),
        ] {
            if on {
                attrs = attrs | attr;
            }
        }
        attrs = attrs
            | match style.underline {
                Underline::None => CellAttrs::empty(),
                Underline::Single => CellAttrs::UNDERLINE,
                Underline::Double => CellAttrs::DOUBLE_UNDERLINE,
                Underline::Curly => CellAttrs::UNDERCURL,
                Underline::Dotted => CellAttrs::DOTTED_UNDERLINE,
                Underline::Dashed => CellAttrs::DASHED_UNDERLINE,
            };
        CursorTemplate {
            fg: read::style_color(style.fg, NamedColor::Foreground),
            bg: read::style_color(style.bg, NamedColor::Background),
            attrs,
            underline_color: match style.underline_color {
                StyleColor::Default => None,
                color => Some(read::style_color(color, NamedColor::Foreground)),
            },
            // libghostty does not expose the active OSC 8 link of the cursor's pen.
            hyperlink: None,
        }
    }

    fn title(&self) -> Option<String> {
        self.effects.title.borrow().clone()
    }

    fn bell_count(&self) -> u64 {
        self.effects.bells.get()
    }

    fn clipboard_event_count(&self) -> usize {
        self.effects.clipboard.borrow().len()
    }

    fn drain_clipboard_events(&mut self) -> Vec<TerminalClipboardEvent> {
        std::mem::take(&mut *self.effects.clipboard.borrow_mut())
    }

    fn row(&self, absolute_line: usize) -> GhosttyRow {
        let y = u32::try_from(self.hidden() + absolute_line).unwrap_or(u32::MAX);
        read::row(
            &self.terminal,
            y,
            self.terminal.cols(),
            &mut self.scratch.borrow_mut(),
        )
    }

    /// One incremental libghostty compression step over eligible history pages. Contents never
    /// change; reads decompress what they touch.
    fn compress_idle(&mut self) -> bool {
        use super::super::super::ghostty::{CompressionMode, CompressionResult};
        let activity = self.terminal.compression_activity();
        if self.compressed_at == Some(activity) {
            return false;
        }
        match self.terminal.compress(CompressionMode::Incremental) {
            Ok(CompressionResult::Pending) => true,
            Ok(CompressionResult::Complete | CompressionResult::Unsupported) | Err(_) => {
                self.compressed_at = Some(self.terminal.compression_activity());
                false
            }
        }
    }

    fn row_wrapped(&self, absolute_line: usize) -> bool {
        let y = u32::try_from(self.hidden() + absolute_line).unwrap_or(u32::MAX);
        self.terminal
            .row_wraps(Point::Screen { x: 0, y })
            .unwrap_or(false)
    }

    /// libghostty reads only the active screen. Switching away from the alternate screen with
    /// `DECRST 47` and back clears it, so its contents and cursor are restored from a repaint.
    fn with_primary_screen<R>(&mut self, read: impl FnOnce(&Self) -> R) -> R {
        if !self.alternate() {
            return read(self);
        }
        // Damage up to here is real; what the switches cause is not.
        self.collect_damage();
        let (rows, cols) = (self.terminal.rows(), self.terminal.cols());
        let mut repair = Vec::new();
        let repainted =
            super::super::replay::write_grid_repaint(self, &mut repair, false, |_, _, _| Ok(()))
                .and_then(|()| super::super::replay::write_cursor(self, &mut repair, rows, cols));
        debug_assert!(repainted.is_ok(), "writing into a Vec cannot fail");
        // Lost on the way: the alternate screen's saved cursor (DECSC) and a pending wrap, which
        // libghostty offers no way to set.
        let responses = self.effects.responses.borrow().len();
        // The copy of the live rows is the alternate screen's; `read` must not see it.
        self.scratch.get_mut().invalidate();
        self.terminal.write(b"\x1b[?47l");
        let result = read(self);
        self.terminal.write(b"\x1b[?47h");
        self.terminal.write(&repair);
        self.effects.responses.borrow_mut().truncate(responses);
        let _ = self.render.update(&mut self.terminal);
        self.render.clean();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a suffix that can still become a special sequence is held, and nothing once it can
    /// no longer.
    #[test]
    fn only_unfinished_special_sequences_are_held() {
        for (bytes, held) in [
            (&b"text\x1b"[..], 1),
            (b"text\x1b[", 2),
            (b"\x1b[3", 3),
            (b"\x1b[?104", 6),
            (b"\x1b[?1049", 7),
            (b"\x1b[?47", 5),
            (b"\x1b[3J", 0),
            (b"\x1b[?1049l", 0),
            (b"\x1b[31", 0),
            (b"\x1b[?1048", 0),
            (b"\x1b]133;A\x1b\\", 0),
            (b"\x1b[?1049lx\x1b[?", 3),
            (b"", 0),
        ] {
            assert_eq!(special_sequence_prefix(bytes), held, "{bytes:?}");
        }
    }

    /// Cutting long runs into pieces must leave exactly the grid one write leaves: graphemes,
    /// wide characters and multi-byte sequences that straddle a piece boundary included.
    #[test]
    fn bounded_text_runs_leave_the_grid_one_write_leaves() {
        let rows: [String; 6] = [
            "字".repeat(150),
            "A😀".repeat(70),
            "e\u{301}x👨\u{200d}👩\u{200d}👧".repeat(25),
            "🇯🇵👍🏽❤\u{fe0f}1\u{fe0f}\u{20e3}".repeat(20),
            format!(
                "{}\x1b[31m{}\x1b[0m{}",
                "Ａ".repeat(80),
                "é".repeat(90),
                "x".repeat(300)
            ),
            "ascii only ".repeat(40),
        ];
        let mut stream = Vec::new();
        for frame in 0..2 {
            for (row, text) in rows.iter().enumerate() {
                stream.extend_from_slice(
                    format!("\x1b[{};{}H{text}", row * 3 + 1, frame + 1).as_bytes(),
                );
            }
        }
        let (cols, height) = (300, 24);
        let mut whole = Terminal::new(cols, height).unwrap();
        whole.write(&stream);
        let mut bounded = Terminal::new(cols, height).unwrap();
        write_text_runs_bounded(&mut bounded, &stream);
        for y in 0..u32::from(height) {
            for x in 0..cols {
                let point = Point::Active { x, y };
                assert_eq!(
                    (bounded.cell_text(point), bounded.cell_wide(point)),
                    (whole.cell_text(point), whole.cell_wide(point)),
                    "cell {x},{y}"
                );
            }
        }
        assert_eq!(bounded.cursor_position(), whole.cursor_position());
    }
}
