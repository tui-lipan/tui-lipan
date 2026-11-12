//! [`TerminalEngine`] over `alacritty_terminal`.
//!
//! Everything Alacritty cannot do the way the engine interface describes is made up for here:
//!
//! - It reports nothing in-band about OSC 7/22/133 or XTVERSION, so a second VT parser
//!   ([`semantic`](super::semantic)) reads the raw bytes ahead of the grid and reports where
//!   anchors belong.
//! - It cannot say how many history lines it evicted, so the grid is driven through
//!   [`ledger::LedgerTerm`], which counts them.
//! - It exposes only the active grid, so reading the primary screen swaps grids and repairs the
//!   alternate one afterwards.
//! - It has no tracked grid points, so a rewrap carries marker characters on the cells to follow.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use alacritty_terminal::event::{Event as TermEvent, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{
    Cell as TermCell, CellExtra as TermCellExtra, Flags as CellFlags, Hyperlink as TermHyperlink,
};
use alacritty_terminal::term::{
    ClipboardType as TermClipboardType, Config as TermConfig, Term, TermDamage, TermMode,
};
use alacritty_terminal::vte::Parser as SemanticVteParser;
use alacritty_terminal::vte::ansi::Processor as VteProcessor;
use alacritty_terminal::vte::ansi::Timeout as _;
use alacritty_terminal::vte::ansi::{
    Color as TermColor, CursorShape as TermCursorShape, CursorStyle as TermCursorStyle,
    NamedColor as TermNamedColor, Rgb as TermRgb,
};

use super::super::super::damage::{DamageAccumulator, TerminalDamage};
use super::super::{
    TerminalCellSize, TerminalClipboardEvent, TerminalClipboardTarget, TerminalColorPalette,
};
use super::{
    AnchorKind, CellAttrs, CellColor, CellRead, CursorAnchor, CursorTemplate, EngineAnchor,
    EngineConfig, EngineCursorShape, EngineCursorStyle, EngineObserver, GridPoint, GridStep,
    LinkRead, ModeFlags, NamedColor, PromptAnchor, ResizeOutcome, ResizeRequest, RowRead,
    TerminalEngine, TrackedPoint,
};

mod ledger;

use super::semantic;

use ledger::{CursorStyleOverrides, HostModes, LedgerTerm, ledger_capacity, settle_history};

// The neutral attribute bits are Alacritty's, so converting a cell's flags is a mask.
const _: () = {
    assert!(CellAttrs::INVERSE.bits() == CellFlags::INVERSE.bits());
    assert!(CellAttrs::BOLD.bits() == CellFlags::BOLD.bits());
    assert!(CellAttrs::ITALIC.bits() == CellFlags::ITALIC.bits());
    assert!(CellAttrs::UNDERLINE.bits() == CellFlags::UNDERLINE.bits());
    assert!(CellAttrs::WIDE_CHAR.bits() == CellFlags::WIDE_CHAR.bits());
    assert!(CellAttrs::WIDE_CHAR_SPACER.bits() == CellFlags::WIDE_CHAR_SPACER.bits());
    assert!(CellAttrs::DIM.bits() == CellFlags::DIM.bits());
    assert!(CellAttrs::HIDDEN.bits() == CellFlags::HIDDEN.bits());
    assert!(CellAttrs::STRIKEOUT.bits() == CellFlags::STRIKEOUT.bits());
    assert!(
        CellAttrs::LEADING_WIDE_CHAR_SPACER.bits() == CellFlags::LEADING_WIDE_CHAR_SPACER.bits()
    );
    assert!(CellAttrs::DOUBLE_UNDERLINE.bits() == CellFlags::DOUBLE_UNDERLINE.bits());
    assert!(CellAttrs::UNDERCURL.bits() == CellFlags::UNDERCURL.bits());
    assert!(CellAttrs::DOTTED_UNDERLINE.bits() == CellFlags::DOTTED_UNDERLINE.bits());
    assert!(CellAttrs::DASHED_UNDERLINE.bits() == CellFlags::DASHED_UNDERLINE.bits());
};

struct TermDimensions {
    rows: usize,
    cols: usize,
}

impl Dimensions for TermDimensions {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// The viewport in both units, which is what a pixel-size report needs.
#[derive(Clone, Copy, Debug, Default)]
struct ViewportGeometry {
    rows: u16,
    cols: u16,
    cell: TerminalCellSize,
}

/// Event listener that captures what `alacritty_terminal` hands its embedder.
///
/// When the parser meets a sequence that requires a response (device attributes, cursor position
/// reports, ...), `alacritty_terminal` generates `Event::PtyWrite`. Those are queued here and
/// handed to the observer in stream order.
#[derive(Clone, Default)]
pub(in super::super) struct ResponseCapture {
    responses: Rc<RefCell<Vec<Vec<u8>>>>,
    /// Decoded OSC 52 store requests for the host application to apply according to its policy.
    clipboard_events: Rc<RefCell<Vec<TerminalClipboardEvent>>>,
    /// Number of BEL events emitted by the terminal parser.
    bell_count: Rc<Cell<u64>>,
    /// Latest window title set by the program via OSC 0/2; `None` once reset.
    title: Rc<RefCell<Option<String>>>,
    /// The palette used to answer `OSC 4/10/11 ; ?` color queries so guest programs don't block
    /// waiting for a reply (see [`Self::resolve_query_color`]).
    palette: Rc<RefCell<TerminalColorPalette>>,
    /// Viewport geometry, used to answer `CSI 14 t` (text-area size in pixels). Programs that draw
    /// images ask this when the PTY reports no pixel dimensions, and block on the reply.
    viewport: Rc<Cell<ViewportGeometry>>,
}

impl ResponseCapture {
    /// Resolve the RGB a color query (`OSC 4/10/11 ; ?`) should report for the
    /// alacritty color slot `index`, using the active palette. Slots are:
    /// `0..16` themed ANSI, `16..256` the standard 256-color cube/grayscale
    /// ramp, `256` foreground, `257`/`268` background, others foreground-ish.
    fn resolve_query_color(&self, index: usize) -> TermRgb {
        use crate::style::Color as UiColor;
        let palette = self.palette.borrow();
        let standard = |i: usize| UiColor::Indexed(i as u8).to_rgb().unwrap_or((0, 0, 0));
        let (r, g, b) = match index {
            0..=15 => palette.ansi[index]
                .to_rgb()
                .unwrap_or_else(|| standard(index)),
            16..=255 => standard(index),
            257 | 268 => palette
                .background
                .and_then(UiColor::to_rgb)
                .unwrap_or((0, 0, 0)),
            _ => palette
                .foreground
                .and_then(UiColor::to_rgb)
                .unwrap_or((255, 255, 255)),
        };
        TermRgb { r, g, b }
    }
}

impl EventListener for ResponseCapture {
    fn send_event(&self, event: TermEvent) {
        match event {
            TermEvent::PtyWrite(text) => self.responses.borrow_mut().push(text.into_bytes()),
            TermEvent::Bell => self.bell_count.set(self.bell_count.get().saturating_add(1)),
            TermEvent::Title(title) => *self.title.borrow_mut() = Some(title),
            TermEvent::ResetTitle => *self.title.borrow_mut() = None,
            TermEvent::ClipboardStore(target, text) => {
                let target = match target {
                    TermClipboardType::Clipboard => TerminalClipboardTarget::Clipboard,
                    TermClipboardType::Selection => TerminalClipboardTarget::Selection,
                };
                self.clipboard_events
                    .borrow_mut()
                    .push(TerminalClipboardEvent { target, text });
            }
            // Answer color queries from the active palette. Without this the
            // guest blocks until its own timeout (e.g. tui-lipan's host-color
            // refresh), since alacritty delegates the reply to the listener.
            TermEvent::ColorRequest(index, formatter) => {
                let response = formatter(self.resolve_query_color(index));
                self.responses.borrow_mut().push(response.into_bytes());
            }
            // Report the text area in pixels. Without this a child that measures itself
            // before drawing an image waits out its own timeout, since alacritty delegates the
            // reply to the listener.
            TermEvent::TextAreaSizeRequest(formatter) => {
                let viewport = self.viewport.get();
                let response = formatter(WindowSize {
                    num_lines: viewport.rows,
                    num_cols: viewport.cols,
                    cell_width: viewport.cell.width,
                    cell_height: viewport.cell.height,
                });
                self.responses.borrow_mut().push(response.into_bytes());
            }
            // OSC 52 loads stay disabled by the parser's default OnlyCopy policy.
            _ => {}
        }
    }
}

/// `alacritty_terminal`, driven through the scrollback ledger.
pub(in super::super) struct AlacrittyEngine {
    processor: VteProcessor,
    pub(in super::super) term: Term<ResponseCapture>,
    listener: ResponseCapture,
    /// Reads raw bytes ahead of `processor` for the sequences the observer interprets; see
    /// [`semantic`](super::semantic).
    semantic_parser: SemanticVteParser,
    default_cursor_style: TermCursorStyle,
    cursor_overrides: CursorStyleOverrides,
    /// Scrollback depth exposed to callers.
    pub(in super::super) scrollback_len: usize,
    /// Grid capacity backing `scrollback_len`, including ledger headroom.
    ledger_capacity: usize,
    /// Whether the child asked for pixel mouse reporting (1016); see [`HostModes`].
    pixel_mouse: bool,
}

fn term_config(capacity: usize, default_cursor_style: TermCursorStyle) -> TermConfig {
    TermConfig {
        scrolling_history: capacity,
        default_cursor_style,
        // Track Kitty keyboard protocol pushes so `key_modes()` can report what the child
        // negotiated; without this alacritty silently drops every `CSI > <flags> u`.
        kitty_keyboard: true,
        ..TermConfig::default()
    }
}

fn term_cursor_style(style: EngineCursorStyle) -> TermCursorStyle {
    TermCursorStyle {
        shape: match style.shape {
            EngineCursorShape::Block => TermCursorShape::Block,
            EngineCursorShape::Underline => TermCursorShape::Underline,
            EngineCursorShape::Beam => TermCursorShape::Beam,
            EngineCursorShape::HollowBlock => TermCursorShape::HollowBlock,
            EngineCursorShape::Hidden => TermCursorShape::Hidden,
        },
        blinking: style.blinking,
    }
}

impl AlacrittyEngine {
    /// Drive the grid parser, returning what the ledger saw.
    fn advance(&mut self, bytes: &[u8]) -> GridStep {
        if bytes.is_empty() {
            return GridStep::default();
        }
        self.with_ledger(|processor, ledger| processor.advance(ledger, bytes))
    }

    /// Drive the term through the scrollback ledger, returning what it saw.
    fn with_ledger(
        &mut self,
        drive: impl FnOnce(&mut VteProcessor, &mut LedgerTerm<'_, ResponseCapture>),
    ) -> GridStep {
        let mut ledger = LedgerTerm::new(
            &mut self.term,
            self.scrollback_len,
            self.ledger_capacity,
            HostModes {
                pixel_mouse: &mut self.pixel_mouse,
                responses: &self.listener.responses,
            },
            &mut self.cursor_overrides,
        );
        drive(&mut self.processor, &mut ledger);
        ledger.step()
    }

    /// Hand queued replies and the grid step to the observer, in that order.
    fn deliver<O: EngineObserver>(
        &mut self,
        step: GridStep,
        observer: &mut O,
        total: &mut GridStep,
    ) {
        let responses = std::mem::take(&mut *self.listener.responses.borrow_mut());
        for response in responses {
            observer.respond(response);
        }
        observer.grid_step(step);
        total.merge(step);
    }

    fn cursor_anchor(&self) -> CursorAnchor {
        let grid = self.term.grid();
        CursorAnchor {
            absolute_line: super::absolute_line(grid.history_size(), grid.cursor.point.line.0),
            column: grid.cursor.point.column.0,
            alt_screen: self.term.mode().contains(TermMode::ALT_SCREEN),
        }
    }

    /// Whether resizing to `new_cols` would move text between lines.
    ///
    /// Only two things can: a line already wrapped into the next one, which widening would pull
    /// back up, and a line reaching past the new width, which narrowing would push down. With
    /// neither present every line keeps exactly the text it has, and so does anything anchored to
    /// it. Worth the scan - the alternative is treating every width change as a rewrap, which in a
    /// tiling multiplexer costs a pane every image in it each time a neighbour opens.
    fn width_change_rewraps(&self, new_cols: u16) -> bool {
        let grid = self.term.grid();
        let last_column = grid.last_column();
        for line in grid.topmost_line().0..=grid.bottommost_line().0 {
            let line = Line(line);
            if grid[line][last_column].flags.contains(CellFlags::WRAPLINE) {
                return true;
            }
            // Anything past the new width has to go somewhere once the grid narrows.
            for column in usize::from(new_cols)..=last_column.0 {
                let cell = &grid[line][Column(column)];
                if cell.c != ' ' || cell.bg != TermColor::Named(TermNamedColor::Background) {
                    return true;
                }
            }
        }
        false
    }

    /// Mark the cell each tracked point is on, so a rewrap carries the mark to wherever that text
    /// goes. Returns what each marked cell held before, by point order; `None` for a point that
    /// could not be marked, which the relocation then drops.
    ///
    /// Line indices cannot be corrected after a rewrap: history is redistributed across a
    /// different number of rows, and lines can be pulled back from or pushed out past scrollback.
    /// The cell itself is the one thing that reliably moves with the text, and a marked cell is
    /// never empty, so the rewrap cannot trim it either.
    fn mark_reflow_anchors(
        &mut self,
        points: &[TrackedPoint],
    ) -> Vec<Option<Option<Arc<TermCellExtra>>>> {
        if points.is_empty() {
            return Vec::new();
        }
        let alt_screen = self.term.mode().contains(TermMode::ALT_SCREEN);
        let grid = self.term.grid_mut();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        let last_column = grid.last_column().0;
        // Points can share a cell; each must restore what the cell held before any of them
        // marked it.
        let mut originals: BTreeMap<(i32, usize), Option<Arc<TermCellExtra>>> = BTreeMap::new();
        points
            .iter()
            .enumerate()
            .map(|(index, point)| {
                // Primary-screen points are not on the grid being shown, and the alternate
                // screen keeps no history to rewrap them against.
                if point.alt_screen != alt_screen {
                    return None;
                }
                let index_mark = reflow_anchor_index_mark(index)?;
                let line = top.checked_add(i32::try_from(point.absolute_line).ok()?)?;
                if line > bottom {
                    return None;
                }
                let column = usize::from(point.column).min(last_column);
                let cell = &mut grid[Line(line)][Column(column)];
                let saved = originals
                    .entry((line, column))
                    .or_insert_with(|| cell.extra.clone())
                    .clone();
                cell.push_zerowidth(REFLOW_ANCHOR_MARK);
                cell.push_zerowidth(index_mark);
                Some(saved)
            })
            .collect()
    }

    /// Find where the rewrap put each mark [`Self::mark_reflow_anchors`] left, restore the cell
    /// it was left on, and report where each point went. A point whose mark is gone - evicted
    /// from scrollback, or never marked - is `None`.
    fn relocate_reflow_anchors(
        &mut self,
        saved: Vec<Option<Option<Arc<TermCellExtra>>>>,
    ) -> Vec<Option<(usize, u16)>> {
        let mut relocated = vec![None; saved.len()];
        let mut remaining = saved.iter().filter(|extra| extra.is_some()).count();
        let grid = self.term.grid_mut();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        let columns = grid.columns();
        let mut line = top;
        while remaining > 0 && line <= bottom {
            let row = &mut grid[Line(line)];
            for column in 0..columns {
                let cell = &mut row[Column(column)];
                let indices = cell
                    .zerowidth()
                    .map_or_else(Vec::new, reflow_anchor_indices);
                for index in indices {
                    let Some(extra) = saved.get(index).and_then(Option::as_ref) else {
                        continue;
                    };
                    cell.extra = extra.clone();
                    if relocated[index].is_none() {
                        relocated[index] = Some(((line - top) as usize, column as u16));
                        remaining = remaining.saturating_sub(1);
                    }
                }
            }
            line += 1;
        }
        relocated
    }

    /// Erase the active semantic prompt and reflow its anchor as the cursor; see
    /// [`ResizeRequest::prompt`].
    fn prepare_prompt_resize(&mut self, mark: PromptAnchor, rows: u16) -> Option<PromptResize> {
        let mut anchor = self.clear_active_prompt(mark)?;
        let cols = self.term.columns();
        // Preserve height scrolling against the shell's actual cursor first.
        self.term.resize(TermDimensions {
            rows: rows as usize,
            cols,
        });
        let grid = self.term.grid();
        let line = grid.cursor.point.line.0 - anchor.cursor_line_offset as i32;
        if line < grid.topmost_line().0 {
            return None;
        }
        let mut logical_start = Line(line);
        while logical_start > grid.topmost_line()
            && grid[Line(logical_start.0 - 1)][grid.last_column()]
                .flags
                .contains(CellFlags::WRAPLINE)
        {
            logical_start -= 1;
        }
        // Alacritty clamps cursor movement at the top of the viewport. Pull a wrapped output
        // prefix and the anchor itself out of history temporarily so reflow can track them.
        anchor.reflow_rows = rows as usize + (-logical_start.0).max(0) as usize;
        self.term.resize(TermDimensions {
            rows: anchor.reflow_rows,
            cols,
        });
        let grid = self.term.grid_mut();
        grid.cursor.point.line.0 -= anchor.cursor_line_offset as i32;
        grid.cursor.point.column = Column(anchor.column);
        grid.cursor.input_needs_wrap = false;
        Some(anchor)
    }

    fn finish_prompt_resize(&mut self, anchor: PromptResize, rows: u16, cols: u16) -> PromptAnchor {
        let cursor = self.cursor_anchor();
        let prompt = PromptAnchor {
            absolute_line: cursor.absolute_line,
            column: cursor.column,
        };
        let grid = self.term.grid_mut();
        grid.cursor.point.line.0 = (grid.cursor.point.line.0 + anchor.cursor_line_offset as i32)
            .min(anchor.reflow_rows as i32 - 1);
        grid.cursor.point.column.0 = anchor.cursor_column.min(cols as usize - 1);
        grid.cursor.input_needs_wrap = false;
        self.term.resize(TermDimensions {
            rows: rows as usize,
            cols: cols as usize,
        });
        prompt
    }

    /// Erase an active semantic prompt before grid reflow.
    ///
    /// Shells receive `SIGWINCH` after a PTY resize and redraw their current prompt. Leaving the
    /// old prompt in the reflow buffer makes that redraw additive, duplicating wrapped input.
    fn clear_active_prompt(&mut self, mark: PromptAnchor) -> Option<PromptResize> {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }
        let rows = self.term.screen_lines();
        let grid = self.term.grid_mut();
        let line = grid.topmost_line().0 + mark.absolute_line as i32;
        let screen_lines = grid.screen_lines() as i32;
        if !(grid.topmost_line().0..screen_lines).contains(&line) {
            return None;
        }

        let columns = grid.columns();
        let start_column = mark.column.min(columns);
        let background = grid.cursor.template.bg;
        for cell in &mut grid[Line(line)][Column(start_column)..Column(columns)] {
            *cell = background.into();
        }
        for row in line + 1..screen_lines {
            for cell in &mut grid[Line(row)][Column(0)..Column(columns)] {
                *cell = background.into();
            }
        }
        // Let Alacritty reflow the prompt anchor as its cursor, including a nonzero column
        // after unterminated output. Restore the shell's physical cursor offset afterward.
        // Tracking the anchor also prevents erased cursor space from creating blank wraps.
        // Readline's resize redraw does not rerun PROMPT_COMMAND, so OSC 133;A is not repeated.
        Some(PromptResize {
            cursor_line_offset: (grid.cursor.point.line.0 - line).max(0) as usize,
            cursor_column: grid.cursor.point.column.0,
            column: start_column,
            reflow_rows: rows,
        })
    }
}

struct PromptResize {
    cursor_line_offset: usize,
    cursor_column: usize,
    column: usize,
    reflow_rows: usize,
}

impl TerminalEngine for AlacrittyEngine {
    type Link = TermHyperlink;
    type Row<'a> = &'a alacritty_terminal::grid::Row<TermCell>;

    fn new(config: EngineConfig) -> Self {
        let dimensions = TermDimensions {
            rows: usize::from(config.rows),
            cols: usize::from(config.cols),
        };
        // Extra headroom above the exposed scrollback so the grid can never saturate inside a
        // single handler call; `LedgerTerm` trims back down to `scrollback` and counts what it
        // dropped. See `ledger`.
        let capacity = ledger_capacity(config.scrollback, config.rows);
        let default_cursor_style = term_cursor_style(config.default_cursor_style);
        let listener = ResponseCapture::default();
        let term = Term::new(
            term_config(capacity, default_cursor_style),
            &dimensions,
            listener.clone(),
        );
        Self {
            processor: VteProcessor::new(),
            term,
            listener,
            semantic_parser: SemanticVteParser::new(),
            default_cursor_style,
            cursor_overrides: CursorStyleOverrides::default(),
            scrollback_len: config.scrollback,
            ledger_capacity: capacity,
            pixel_mouse: false,
        }
    }

    fn reset(&mut self) {
        let dimensions = TermDimensions {
            rows: self.term.screen_lines(),
            cols: self.term.columns(),
        };
        let viewport = self.listener.viewport.get();
        self.listener = ResponseCapture::default();
        // The query palette is deliberately not carried over: a reset has always answered color
        // queries from the default palette until the next palette change.
        self.listener.viewport.set(viewport);
        self.cursor_overrides = CursorStyleOverrides::default();
        self.term = Term::new(
            term_config(self.ledger_capacity, self.default_cursor_style),
            &dimensions,
            self.listener.clone(),
        );
        self.processor = VteProcessor::new();
        // Drop any in-flight partial OSC/CSI sequence.
        self.semantic_parser = SemanticVteParser::new();
        self.pixel_mouse = false;
    }

    fn set_default_cursor_style(&mut self, style: EngineCursorStyle) -> bool {
        let default_cursor_style = term_cursor_style(style);
        if self.default_cursor_style == default_cursor_style {
            return false;
        }
        let child_style = self
            .cursor_overrides
            .resolve(self.term.cursor_style(), default_cursor_style);
        self.default_cursor_style = default_cursor_style;
        self.term
            .set_options(term_config(self.ledger_capacity, default_cursor_style));
        alacritty_terminal::vte::ansi::Handler::set_cursor_style(&mut self.term, child_style);
        true
    }

    fn set_palette(&mut self, palette: &TerminalColorPalette) {
        *self.listener.palette.borrow_mut() = *palette;
    }

    fn set_report_geometry(&mut self, rows: u16, cols: u16, cell: TerminalCellSize) {
        self.listener
            .viewport
            .set(ViewportGeometry { rows, cols, cell });
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
                None,
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

    fn anchor<O: EngineObserver>(&mut self, anchor: O::Anchor, observer: &mut O) -> GridStep {
        let mut total = GridStep::default();
        match anchor.kind() {
            AnchorKind::Order => observer.anchor(anchor, None),
            AnchorKind::Cursor => {
                // VTE buffers grid, cursor, and mode changes between BSU/ESU markers, so a cursor
                // read in the middle of one sees the previous frame. Commit the prefix, read, then
                // resume synchronization for the rest of the frame.
                let synchronized = self.sync_pending();
                if synchronized {
                    let step = self.with_ledger(|processor, ledger| processor.stop_sync(ledger));
                    self.deliver(step, observer, &mut total);
                }
                let cursor = self.cursor_anchor();
                observer.anchor(anchor, Some(cursor));
                if synchronized {
                    let step = self.advance(b"\x1b[?2026h");
                    self.deliver(step, observer, &mut total);
                }
            }
        }
        total
    }

    fn feed<O: EngineObserver>(&mut self, bytes: &[u8], observer: &mut O) -> GridStep {
        let mut total = GridStep::default();
        let step = self.advance(bytes);
        self.deliver(step, observer, &mut total);
        total
    }

    fn sync_pending(&self) -> bool {
        self.processor.sync_timeout().pending_timeout() || self.processor.sync_bytes_count() != 0
    }

    fn commit_sync<O: EngineObserver>(&mut self, observer: &mut O) -> GridStep {
        let mut total = GridStep::default();
        let step = self.with_ledger(|processor, ledger| processor.stop_sync(ledger));
        self.deliver(step, observer, &mut total);
        total
    }

    fn resume_sync<O: EngineObserver>(&mut self, observer: &mut O) -> GridStep {
        let mut total = GridStep::default();
        let step = self.advance(b"\x1b[?2026h");
        self.deliver(step, observer, &mut total);
        total
    }

    /// Taken every write rather than once per frame because `Term::damage` is cleared by reading
    /// it, and several writes can land between two paints.
    fn take_damage(&mut self) -> TerminalDamage {
        let mut completed = DamageAccumulator::default();
        let viewport_rows = self.term.screen_lines();
        match self.term.damage() {
            TermDamage::Full => completed.mark_full(),
            TermDamage::Partial(iter) => {
                for line in iter {
                    completed.add_row(line.line, line.left, line.right, viewport_rows);
                }
            }
        }
        self.term.reset_damage();
        completed.take()
    }

    fn resize(&mut self, request: ResizeRequest<'_>) -> ResizeOutcome {
        let ResizeRequest {
            rows,
            cols,
            prompt,
            track,
        } = request;
        let reflowed = usize::from(cols) != self.term.columns();
        let prompt_resize = prompt.and_then(|mark| self.prepare_prompt_resize(mark, rows));
        let dimensions = TermDimensions {
            rows: prompt_resize
                .as_ref()
                .map_or(rows as usize, |anchor| anchor.reflow_rows),
            cols: cols as usize,
        };
        let reflow_marks = match track {
            Some(points) if reflowed && self.width_change_rewraps(cols) => {
                Some(self.mark_reflow_anchors(points))
            }
            _ => None,
        };
        self.term.resize(dimensions);
        let prompt = prompt_resize.map(|anchor| self.finish_prompt_resize(anchor, rows, cols));
        // `Term::resize` can push lines into history without going through a handler
        // call, so the ledger never sees it; trim and account for it here.
        self.ledger_capacity = ledger_capacity(self.scrollback_len, rows);
        let evicted = settle_history(&mut self.term, self.scrollback_len, self.ledger_capacity);
        let relocated = reflow_marks.map(|marks| self.relocate_reflow_anchors(marks));
        ResizeOutcome {
            evicted,
            prompt,
            relocated,
        }
    }

    fn screen_lines(&self) -> usize {
        self.term.screen_lines()
    }

    fn columns(&self) -> usize {
        self.term.columns()
    }

    fn history_size(&self) -> usize {
        self.term.history_size()
    }

    fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    fn scroll_display_to(&mut self, offset: usize) {
        let target = offset.min(self.term.history_size());
        let current = self.term.grid().display_offset();
        let delta = target as i32 - current as i32;
        if delta != 0 {
            self.term.scroll_display(Scroll::Delta(delta));
        }
    }

    fn modes(&self) -> ModeFlags {
        let mode = *self.term.mode();
        let mut flags = ModeFlags::empty();
        for (term, flag) in [
            (TermMode::SHOW_CURSOR, ModeFlags::SHOW_CURSOR),
            (TermMode::APP_CURSOR, ModeFlags::APP_CURSOR),
            (TermMode::APP_KEYPAD, ModeFlags::APP_KEYPAD),
            (TermMode::MOUSE_REPORT_CLICK, ModeFlags::MOUSE_REPORT_CLICK),
            (TermMode::BRACKETED_PASTE, ModeFlags::BRACKETED_PASTE),
            (TermMode::SGR_MOUSE, ModeFlags::SGR_MOUSE),
            (TermMode::MOUSE_MOTION, ModeFlags::MOUSE_MOTION),
            (TermMode::LINE_WRAP, ModeFlags::LINE_WRAP),
            (TermMode::FOCUS_IN_OUT, ModeFlags::FOCUS_IN_OUT),
            (TermMode::ALT_SCREEN, ModeFlags::ALT_SCREEN),
            (TermMode::MOUSE_DRAG, ModeFlags::MOUSE_DRAG),
            (TermMode::UTF8_MOUSE, ModeFlags::UTF8_MOUSE),
            (
                TermMode::DISAMBIGUATE_ESC_CODES,
                ModeFlags::DISAMBIGUATE_ESC_CODES,
            ),
            (TermMode::REPORT_EVENT_TYPES, ModeFlags::REPORT_EVENT_TYPES),
            (
                TermMode::REPORT_ALTERNATE_KEYS,
                ModeFlags::REPORT_ALTERNATE_KEYS,
            ),
            (
                TermMode::REPORT_ALL_KEYS_AS_ESC,
                ModeFlags::REPORT_ALL_KEYS_AS_ESC,
            ),
            (
                TermMode::REPORT_ASSOCIATED_TEXT,
                ModeFlags::REPORT_ASSOCIATED_TEXT,
            ),
        ] {
            flags.set(flag, mode.contains(term));
        }
        flags.set(ModeFlags::SGR_PIXELS_MOUSE, self.pixel_mouse);
        flags
    }

    fn mode_fingerprint(&self) -> u64 {
        u64::from(self.term.mode().bits())
    }

    fn cursor(&self) -> GridPoint {
        let point = self.term.grid().cursor.point;
        GridPoint {
            line: point.line.0,
            column: point.column.0,
        }
    }

    fn display_cursor(&self) -> GridPoint {
        let mut point = self.term.grid().cursor.point;
        if self.term.grid()[point]
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            point.column -= 1;
        }
        GridPoint {
            line: point.line.0,
            column: point.column.0,
        }
    }

    fn cursor_style(&self) -> EngineCursorStyle {
        let style = self.term.cursor_style();
        EngineCursorStyle {
            shape: match style.shape {
                TermCursorShape::Block => EngineCursorShape::Block,
                TermCursorShape::Underline => EngineCursorShape::Underline,
                TermCursorShape::Beam => EngineCursorShape::Beam,
                TermCursorShape::HollowBlock => EngineCursorShape::HollowBlock,
                TermCursorShape::Hidden => EngineCursorShape::Hidden,
            },
            blinking: style.blinking,
        }
    }

    fn cursor_color(&self) -> Option<(u8, u8, u8)> {
        self.term.colors()[TermNamedColor::Cursor].map(|rgb| (rgb.r, rgb.g, rgb.b))
    }

    fn cursor_template(&self) -> CursorTemplate<TermHyperlink> {
        let template = &self.term.grid().cursor.template;
        CursorTemplate {
            fg: CellRead::fg(template),
            bg: CellRead::bg(template),
            attrs: CellRead::attrs(template),
            underline_color: CellRead::underline_color(template),
            hyperlink: CellRead::hyperlink(template),
        }
    }

    fn title(&self) -> Option<String> {
        self.listener.title.borrow().clone()
    }

    fn bell_count(&self) -> u64 {
        self.listener.bell_count.get()
    }

    fn clipboard_event_count(&self) -> usize {
        self.listener.clipboard_events.borrow().len()
    }

    fn drain_clipboard_events(&mut self) -> Vec<TerminalClipboardEvent> {
        std::mem::take(&mut *self.listener.clipboard_events.borrow_mut())
    }

    fn row(&self, absolute_line: usize) -> Self::Row<'_> {
        let grid = self.term.grid();
        &grid[Line(grid.topmost_line().0 + absolute_line as i32)]
    }

    /// `alacritty_terminal` only exposes the active grid. Switching back to the alternate screen
    /// clears it, so its contents are restored from a synthesized repaint.
    fn with_primary_screen<R>(&mut self, read: impl FnOnce(&Self) -> R) -> R {
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            return read(self);
        }
        let saved_selection = self.term.selection.clone();
        let saved_alt_cursor = self.term.grid().cursor.clone();
        let saved_alt_saved_cursor = self.term.grid().saved_cursor.clone();
        let mut alt_repaint = Vec::new();
        let repainted =
            super::super::replay::write_grid_repaint(self, &mut alt_repaint, false, |_, _, _| {
                Ok(())
            });
        debug_assert!(repainted.is_ok(), "writing into a Vec cannot fail");
        self.term.swap_alt();
        let result = read(self);

        self.term.swap_alt();
        let responses = self.listener.responses.borrow().len();
        let mut repair_processor: VteProcessor = VteProcessor::new();
        repair_processor.advance(&mut self.term, &alt_repaint);
        // The repaint answers nothing the child asked.
        self.listener.responses.borrow_mut().truncate(responses);
        self.term.grid_mut().cursor = saved_alt_cursor;
        self.term.grid_mut().saved_cursor = saved_alt_saved_cursor;
        self.term.selection = saved_selection;
        // Both swaps and the repair repaint mark emulator damage even though the grids end up
        // unchanged. Real PTY damage was already folded into the screen by the processing path
        // that produced it.
        self.term.reset_damage();
        result
    }
}

impl LinkRead for TermHyperlink {
    #[inline]
    fn uri(&self) -> &str {
        TermHyperlink::uri(self)
    }
}

#[inline]
fn cell_color(color: TermColor) -> CellColor {
    match color {
        TermColor::Named(named) => CellColor::Named(named_color(named)),
        TermColor::Indexed(index) => CellColor::Indexed(index),
        TermColor::Spec(TermRgb { r, g, b }) => CellColor::Rgb(r, g, b),
    }
}

#[inline]
fn named_color(color: TermNamedColor) -> NamedColor {
    match color {
        TermNamedColor::Black => NamedColor::Black,
        TermNamedColor::Red => NamedColor::Red,
        TermNamedColor::Green => NamedColor::Green,
        TermNamedColor::Yellow => NamedColor::Yellow,
        TermNamedColor::Blue => NamedColor::Blue,
        TermNamedColor::Magenta => NamedColor::Magenta,
        TermNamedColor::Cyan => NamedColor::Cyan,
        TermNamedColor::White => NamedColor::White,
        TermNamedColor::BrightBlack => NamedColor::BrightBlack,
        TermNamedColor::BrightRed => NamedColor::BrightRed,
        TermNamedColor::BrightGreen => NamedColor::BrightGreen,
        TermNamedColor::BrightYellow => NamedColor::BrightYellow,
        TermNamedColor::BrightBlue => NamedColor::BrightBlue,
        TermNamedColor::BrightMagenta => NamedColor::BrightMagenta,
        TermNamedColor::BrightCyan => NamedColor::BrightCyan,
        TermNamedColor::BrightWhite => NamedColor::BrightWhite,
        TermNamedColor::Foreground => NamedColor::Foreground,
        TermNamedColor::Background => NamedColor::Background,
        TermNamedColor::Cursor => NamedColor::Cursor,
        TermNamedColor::DimBlack => NamedColor::DimBlack,
        TermNamedColor::DimRed => NamedColor::DimRed,
        TermNamedColor::DimGreen => NamedColor::DimGreen,
        TermNamedColor::DimYellow => NamedColor::DimYellow,
        TermNamedColor::DimBlue => NamedColor::DimBlue,
        TermNamedColor::DimMagenta => NamedColor::DimMagenta,
        TermNamedColor::DimCyan => NamedColor::DimCyan,
        TermNamedColor::DimWhite => NamedColor::DimWhite,
        TermNamedColor::BrightForeground => NamedColor::BrightForeground,
        TermNamedColor::DimForeground => NamedColor::DimForeground,
    }
}

impl CellRead for TermCell {
    type Link = TermHyperlink;

    #[inline]
    fn ch(&self) -> char {
        self.c
    }

    #[inline]
    fn zerowidth(&self) -> &[char] {
        TermCell::zerowidth(self).unwrap_or(&[])
    }

    #[inline]
    fn fg(&self) -> CellColor {
        cell_color(self.fg)
    }

    #[inline]
    fn bg(&self) -> CellColor {
        cell_color(self.bg)
    }

    #[inline]
    fn underline_color(&self) -> Option<CellColor> {
        TermCell::underline_color(self).map(cell_color)
    }

    #[inline]
    fn attrs(&self) -> CellAttrs {
        CellAttrs::from_bits_truncate(self.flags.bits())
    }

    #[inline]
    fn hyperlink(&self) -> Option<TermHyperlink> {
        TermCell::hyperlink(self)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        alacritty_terminal::grid::GridCell::is_empty(self)
    }
}

impl RowRead for &alacritty_terminal::grid::Row<TermCell> {
    type Cell = TermCell;

    #[inline]
    fn cells(&self) -> &[TermCell] {
        &self[Column(0)..]
    }

    #[inline]
    fn wrapped(&self) -> bool {
        self.cells()
            .last()
            .is_some_and(|cell| cell.flags.contains(CellFlags::WRAPLINE))
    }
}

/// Leads the marks a rewrapping resize leaves on tracked cells. A noncharacter, so no program can
/// have written it into a cell's zero-width characters.
const REFLOW_ANCHOR_MARK: char = '\u{FDD0}';

/// Where the point index after a [`REFLOW_ANCHOR_MARK`] counts from: supplementary private-use
/// plane A, which holds 65534 values.
const REFLOW_ANCHOR_INDEX_BASE: u32 = 0xF0000;

fn reflow_anchor_index_mark(index: usize) -> Option<char> {
    let index = u32::try_from(index).ok().filter(|index| *index < 0xFFFE)?;
    char::from_u32(REFLOW_ANCHOR_INDEX_BASE + index)
}

/// The point indices marked on a cell, one per [`REFLOW_ANCHOR_MARK`].
fn reflow_anchor_indices(zerowidth: &[char]) -> Vec<usize> {
    zerowidth
        .windows(2)
        .filter(|pair| pair[0] == REFLOW_ANCHOR_MARK)
        .filter_map(|pair| u32::from(pair[1]).checked_sub(REFLOW_ANCHOR_INDEX_BASE))
        .filter_map(|index| usize::try_from(index).ok())
        .collect()
}
