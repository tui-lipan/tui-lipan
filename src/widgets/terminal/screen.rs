use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::Arc;

use alacritty_terminal::event::{Event as TermEvent, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, GridCell, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags as CellFlags};
use alacritty_terminal::term::{
    self, ClipboardType as TermClipboardType, Config as TermConfig, Term, TermMode,
};
use alacritty_terminal::vte::Parser as SemanticVteParser;
use alacritty_terminal::vte::ansi::Processor as VteProcessor;
use alacritty_terminal::vte::ansi::{
    Color as TermColor, CursorShape as TermCursorShape, CursorStyle as TermCursorStyle, NamedColor,
    Rgb as TermRgb,
};

use super::events::{
    KittyKeyboardFlags, MouseEncoding, MouseMode, MouseModeState, TerminalKeyModes,
    terminal_selection_text_with,
};
#[cfg(feature = "terminal-images")]
use super::graphics::{
    GraphicsCommand, GraphicsContext, GraphicsScanner, GraphicsSegment, PLACEHOLDER,
    PlaceholderCell, TerminalGraphics, TerminalImagePlacement,
};
use super::osc::{
    SemanticObserver, TerminalCommandPhase, TerminalSemanticEvent, TerminalSemanticState,
};
use super::scrollback_ledger::{
    HostModes, LedgerTerm, SGR_PIXELS_MOUSE, ledger_capacity, settle_history,
};
use super::selection::{ScrollbackLineage, TerminalSelection};
use crate::style::{CaretShape, Color as UiColor, HostTerminalColors, Span, Style, Theme};
use crate::utils::{GridPos, GridSelection, SelectionEnd};

/// Kind of semantic mark anchored to an absolute text line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticMarkKind {
    /// `OSC 133;A` — shell drawing a prompt.
    Prompt,
    /// `OSC 133;C` — command output started.
    OutputStart,
    /// `OSC 133;D` — command output ended.
    OutputEnd,
}

/// Clipboard destination requested by a child through OSC 52.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalClipboardTarget {
    /// The regular clipboard (`OSC 52;c`).
    Clipboard,
    /// The primary selection (`OSC 52;p` or `OSC 52;s`).
    Selection,
}

/// A decoded request from a child to store text in a clipboard through OSC 52.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalClipboardEvent {
    /// Clipboard destination requested by the child.
    pub target: TerminalClipboardTarget,
    /// Decoded UTF-8 text supplied by the child.
    pub text: String,
}

/// A semantic mark recorded against the absolute text-line space used by
/// [`TerminalScreen::total_text_lines`] / [`TerminalScreen::export_text`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticMark {
    /// Prompt / output-start / output-end.
    pub kind: SemanticMarkKind,
    /// Absolute line index (`0` = oldest retained history line).
    pub absolute_line: usize,
    /// Exit status from `OSC 133;D`, when present.
    pub exit_status: Option<i32>,
}

const MAX_SEMANTIC_MARKS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromptBoundary {
    Start,
    CommandStart,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PromptScanState {
    #[default]
    Ground,
    Escape,
    Osc,
    OscEscape,
}

/// Finds prompt lifecycle boundaries without delaying the grid parser.
///
/// The semantic parser records state after processing a complete PTY chunk, which is too late for
/// a prompt mark: one chunk commonly contains `OSC 133;A` followed by the prompt and part of the
/// input. This scanner reports the byte offset of `A`/`C` terminators so the grid can be advanced
/// only that far before the cursor position is captured.
#[derive(Debug, Default)]
struct PromptBoundaryScanner {
    state: PromptScanState,
    prefix_len: u8,
    boundary: Option<PromptBoundary>,
}

impl PromptBoundaryScanner {
    fn scan(&mut self, bytes: &[u8]) -> Vec<(usize, PromptBoundary)> {
        if self.state == PromptScanState::Ground && !bytes.contains(&0x1b) && !bytes.contains(&0x9d)
        {
            return Vec::new();
        }

        let mut boundaries = Vec::new();
        for (index, &byte) in bytes.iter().enumerate() {
            match self.state {
                PromptScanState::Ground => match byte {
                    0x1b => self.state = PromptScanState::Escape,
                    0x9d => self.start_osc(),
                    _ => {}
                },
                PromptScanState::Escape => {
                    if byte == b']' {
                        self.start_osc();
                    } else if byte != 0x1b {
                        self.state = PromptScanState::Ground;
                    }
                }
                PromptScanState::Osc => match byte {
                    0x07 | 0x9c => {
                        if let Some(boundary) = self.finish_osc() {
                            boundaries.push((index + 1, boundary));
                        }
                    }
                    0x18 | 0x1a => self.cancel_osc(),
                    0x1b => self.state = PromptScanState::OscEscape,
                    _ => self.consume_osc_byte(byte),
                },
                PromptScanState::OscEscape => match byte {
                    b'\\' => {
                        if let Some(boundary) = self.finish_osc() {
                            boundaries.push((index + 1, boundary));
                        }
                    }
                    0x18 | 0x1a => self.cancel_osc(),
                    0x1b => self.boundary = None,
                    _ => {
                        self.boundary = None;
                        self.state = PromptScanState::Osc;
                    }
                },
            }
        }
        boundaries
    }

    fn start_osc(&mut self) {
        self.state = PromptScanState::Osc;
        self.prefix_len = 0;
        self.boundary = None;
    }

    fn consume_osc_byte(&mut self, byte: u8) {
        let expected = b"133;";
        if usize::from(self.prefix_len) < expected.len() {
            if byte == expected[usize::from(self.prefix_len)] {
                self.prefix_len += 1;
            } else {
                self.prefix_len = u8::MAX;
            }
        } else if self.prefix_len == expected.len() as u8 {
            self.boundary = match byte {
                b'A' => Some(PromptBoundary::Start),
                b'C' => Some(PromptBoundary::CommandStart),
                _ => None,
            };
            self.prefix_len += 1;
        }
    }

    fn finish_osc(&mut self) -> Option<PromptBoundary> {
        let boundary = self.boundary.take();
        self.state = PromptScanState::Ground;
        self.prefix_len = 0;
        boundary
    }

    fn cancel_osc(&mut self) {
        self.state = PromptScanState::Ground;
        self.prefix_len = 0;
        self.boundary = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ActivePromptMark {
    absolute_line: usize,
    column: usize,
}

/// Cursor style applied when the child program never issues `DECSCUSR`.
///
/// A blinking block matches the historical default and the common terminal
/// baseline; explicit `CSI Ps SP q` sequences from the child override it.
const DEFAULT_CURSOR_STYLE: TermCursorStyle = TermCursorStyle {
    shape: TermCursorShape::Block,
    blinking: true,
};

/// Map an `alacritty_terminal` cursor shape to the framework [`CaretShape`].
///
/// `HollowBlock`/`Hidden` collapse to `Block`; visibility is tracked separately
/// via `cursor_visible`.
fn caret_shape_from_term(shape: TermCursorShape) -> CaretShape {
    match shape {
        TermCursorShape::Underline => CaretShape::Underline,
        TermCursorShape::Beam => CaretShape::Bar,
        TermCursorShape::Block | TermCursorShape::HollowBlock | TermCursorShape::Hidden => {
            CaretShape::Block
        }
    }
}

/// Terminal viewport dimensions in character cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalViewport {
    /// Visible columns in the terminal viewport.
    pub cols: u16,
    /// Visible rows in the terminal viewport.
    pub rows: u16,
}

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

/// Event listener that captures PtyWrite events for forwarding to the PTY.
///
/// Cell size in pixels, as the host draws it.
///
/// A terminal's own contents are cells, but programs that draw pictures need pixels: they read the
/// PTY's `TIOCGWINSZ` pixel fields or ask with `CSI 14 t`, then size their output against the
/// answer. Both are reported from the value installed with
/// [`TerminalScreen::set_cell_size`](TerminalScreen::set_cell_size), so what a child computes and
/// what this screen lays out agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalCellSize {
    /// Cell width in pixels.
    pub width: u16,
    /// Cell height in pixels.
    pub height: u16,
}

impl Default for TerminalCellSize {
    /// 10x20, the same guess the image encoder falls back to when the host answers no size query.
    ///
    /// Apps that can measure the host should install
    /// [`host_cell_size`](crate::host_cell_size) instead of relying on this.
    fn default() -> Self {
        Self::new(10, 20)
    }
}

impl TerminalCellSize {
    /// Clamp both axes to at least one pixel.
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
        }
    }
}

/// When the terminal parser encounters escape sequences that require a response
/// (e.g., device attributes queries, cursor position reports), alacritty_terminal
/// generates `Event::PtyWrite` events. This listener captures those responses
/// so they can be written back to the PTY.
#[derive(Clone, Default)]
struct ResponseCapture {
    responses: Rc<RefCell<Vec<Vec<u8>>>>,
    /// Decoded OSC 52 store requests for the host application to apply according to its policy.
    clipboard_events: Rc<RefCell<Vec<TerminalClipboardEvent>>>,
    /// Number of BEL events emitted by the terminal parser.
    bell_count: Rc<Cell<u64>>,
    /// Latest window title set by the program via OSC 0/2; `None` once reset.
    title: Rc<RefCell<Option<String>>>,
    /// The active palette, shared with [`TerminalScreen`], used to answer
    /// `OSC 4/10/11 ; ?` color queries so guest programs don't block waiting
    /// for a reply (see [`Self::resolve_query_color`]).
    palette: Rc<RefCell<TerminalColorPalette>>,
    /// Viewport geometry, shared with [`TerminalScreen`], used to answer `CSI 14 t` (text-area
    /// size in pixels). Programs that draw images ask this when the PTY reports no pixel
    /// dimensions, and block on the reply.
    viewport: Rc<Cell<ViewportGeometry>>,
}

/// The viewport in both units, which is what a pixel-size report needs.
#[derive(Clone, Copy, Debug, Default)]
struct ViewportGeometry {
    rows: u16,
    cols: u16,
    cell: TerminalCellSize,
}

impl ResponseCapture {
    /// Resolve the RGB a color query (`OSC 4/10/11 ; ?`) should report for the
    /// alacritty color slot `index`, using the active palette. Slots are:
    /// `0..16` themed ANSI, `16..256` the standard 256-color cube/grayscale
    /// ramp, `256` foreground, `257`/`268` background, others foreground-ish.
    fn resolve_query_color(&self, index: usize) -> TermRgb {
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

/// Alacritty terminal screen parser for PTY output.
pub struct TerminalScreen {
    processor: VteProcessor,
    term: Term<ResponseCapture>,
    listener: ResponseCapture,
    /// Parallel OSC 7/9;9/133 observer, driven by the same raw bytes as `processor`.
    ///
    /// Kept entirely separate from the Alacritty grid parser above: it never sees a callback
    /// besides `osc_dispatch`, so it cannot affect rendering, and its state is deliberately not
    /// part of `TerminalRenderSnapshot`.
    semantic_parser: SemanticVteParser,
    semantic: SemanticObserver,
    /// Logical viewport rows (matches the PTY size).
    rows: u16,
    /// Logical viewport cols (matches the PTY size).
    cols: u16,
    scrollback_len: usize,
    /// Grid capacity backing `scrollback_len`, including ledger headroom.
    ledger_capacity: usize,
    mouse_mode: MouseModeState,
    /// Whether the child asked for pixel mouse reporting (1016); see [`HostModes`].
    pixel_mouse: bool,
    scrollback_offset: usize,
    cache: TerminalRenderSnapshot,
    palette: TerminalColorPalette,
    dirty: bool,
    sequence: u64,
    /// Bounded history of OSC 133 marks anchored to absolute text lines.
    semantic_marks: VecDeque<SemanticMark>,
    /// How many semantic events have already been turned into marks (reset on drain).
    semantic_events_seen: usize,
    /// Lightweight OSC scanner used to capture prompt marks at their exact grid position.
    prompt_boundary_scanner: PromptBoundaryScanner,
    /// Start of the prompt currently being edited, cleared when command execution begins.
    active_prompt_mark: Option<ActivePromptMark>,
    /// Splits `APC _G` graphics commands out of the byte stream before the grid parser sees it.
    #[cfg(feature = "terminal-images")]
    graphics_scanner: GraphicsScanner,
    /// Decoded images and their placements, anchored to the same absolute lines as the marks.
    #[cfg(feature = "terminal-images")]
    graphics: TerminalGraphics,
    /// Whether the alternate screen was active after the last chunk, so leaving it can drop the
    /// placements that belonged to it.
    #[cfg(feature = "terminal-images")]
    graphics_alt_screen: bool,
    /// Cumulative scrollback lines evicted since creation.
    evicted_lines: u64,
    /// Bumped when absolute line indices are invalidated.
    history_epoch: u64,
    /// Whether the alternate screen was active after the last update.
    alt_screen: bool,
    /// Host cell size, reported to the child and used to size image placements.
    cell_size: TerminalCellSize,
}

/// A [`TerminalScreen`] an app owns and lets the widget read for itself.
///
/// Handing the widget a handle instead of a [`TerminalRenderSnapshot`] takes the screen's contents
/// out of the element tree, which is what lets new output be a repaint rather than a rebuild: the
/// element a `view()` produces no longer changes when the child program draws, so an app can answer
/// output with [`Update::paint`] and the runtime pulls the current snapshot on its way to the
/// screen. Without this, every chunk of terminal output forces `view()` + layout for the whole
/// window — which for a multiplexer means the cost of one pane streaming is paid by all of them.
///
/// [`Update::paint`]: crate::Update::paint
#[derive(Clone)]
pub struct TerminalScreenHandle(Rc<RefCell<TerminalScreen>>);

impl TerminalScreenHandle {
    /// Share `screen` with the widget.
    pub fn new(screen: Rc<RefCell<TerminalScreen>>) -> Self {
        Self(screen)
    }

    /// The screen's current snapshot, rebuilding it only if the screen took output since the last
    /// call (see [`TerminalScreen::render_snapshot`]).
    pub fn snapshot(&self) -> TerminalRenderSnapshot {
        self.0.borrow_mut().render_snapshot()
    }

    /// The cell size in pixels this screen reports to its child (see
    /// [`TerminalScreen::set_cell_size`]).
    pub fn cell_size(&self) -> TerminalCellSize {
        self.0.borrow().cell_size()
    }

    /// Whether the screen currently retains any Kitty graphics (see
    /// [`TerminalScreen::has_images`]).
    #[cfg(feature = "terminal-images")]
    pub fn has_images(&self) -> bool {
        self.0.borrow().has_images()
    }

    /// Extract selected text across retained scrollback using display columns.
    pub fn selection_display_text(
        &self,
        sel: &TerminalSelection,
        endpoint: SelectionEnd,
        trim_row_end: bool,
    ) -> String {
        self.0
            .borrow()
            .selection_display_text(sel, endpoint, trim_row_end)
    }
}

/// Identity, not contents: two handles are the same handle when they share one screen. Comparing
/// contents would defeat the purpose, since the point is an element that holds still while the
/// screen behind it moves.
impl PartialEq for TerminalScreenHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for TerminalScreenHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalScreenHandle")
            .finish_non_exhaustive()
    }
}

impl From<Rc<RefCell<TerminalScreen>>> for TerminalScreenHandle {
    fn from(screen: Rc<RefCell<TerminalScreen>>) -> Self {
        Self::new(screen)
    }
}

/// Renderable terminal snapshot from `TerminalScreen`.
#[derive(Clone, Debug)]
pub struct TerminalRenderSnapshot {
    /// Plain visible contents.
    pub text: Arc<str>,
    /// Styled lines matching `text` logical lines.
    pub color_lines: Arc<[Vec<Span>]>,
    /// Cursor row in the visible viewport.
    pub cursor_row: u16,
    /// Cursor column in the visible viewport.
    pub cursor_col: u16,
    /// Whether cursor should be displayed.
    pub cursor_visible: bool,
    /// Shape the child program requested for the cursor (via `DECSCUSR`).
    pub cursor_shape: CaretShape,
    /// Whether the child program requested a blinking cursor (via `DECSCUSR`).
    pub cursor_blinking: bool,
    /// Stable sequence key for cache invalidation.
    pub sequence: u64,
    /// Current scrollback offset (0 = live view, >0 = scrolled into history).
    pub scrollback_offset: usize,
    /// Total number of scrollback rows available.
    pub total_scrollback_rows: usize,
    /// Cumulative scrollback lines evicted since creation.
    pub evicted_lines: u64,
    /// Bumped when absolute line indices are invalidated.
    pub history_epoch: u64,
    /// Current mouse mode state.
    pub mouse_mode: MouseModeState,
    /// Input-affecting DEC private modes the child has enabled (DECCKM, bracketed paste).
    pub key_modes: TerminalKeyModes,
    /// Images overlapping the visible rows, back to front.
    ///
    /// Positions are viewport-relative and may start above or left of it, so a partly scrolled
    /// image reports the rect it would occupy in full and the renderer crops the pixels.
    #[cfg(feature = "terminal-images")]
    pub images: Arc<[TerminalImagePlacement]>,
    /// Whether each visible row soft-wraps into the row below it.
    ///
    /// A row is one grid row, not one logical line: a program that printed a line longer than the
    /// terminal is wide occupies several of them. Anything scanning [`Self::text`] for something
    /// that can outrun the width - a URL, a path - has to rejoin those rows first, and this is
    /// the only record of where a break was the terminal's rather than the program's. Indexed by
    /// visible row; may be shorter than the viewport, in which case the missing rows do not wrap.
    pub wrapped_rows: Arc<[bool]>,
}

/// A display-column decoration applied to a terminal render snapshot.
///
/// Decorations affect only [`TerminalRenderSnapshot::color_lines`]. The snapshot's plain `text`
/// remains unchanged so callers that scan plain snapshot text continue to see the terminal's
/// original contents. Use [`Self::highlight`] for a restyled range, [`Self::label`] for a span
/// inserted between existing columns, and [`Self::overlay`] for one painted over them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TerminalDecoration {
    row: usize,
    start_col: usize,
    end_col: usize,
    style: Style,
    text: Option<DecorationText>,
}

/// A span a decoration draws, and how it makes room for it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum DecorationText {
    /// Pushed in, moving every later column right.
    Insert(Span),
    /// Painted over the columns it covers, moving nothing.
    Overlay(Span),
}

impl TerminalDecoration {
    /// Highlight the half-open display-column range `cols` on `row`.
    pub fn highlight(row: usize, cols: std::ops::Range<usize>, style: Style) -> Self {
        Self {
            row,
            start_col: cols.start,
            end_col: cols.end,
            style,
            text: None,
        }
    }

    /// Insert a styled label at a display column on `row`, shifting the rest of the row right.
    ///
    /// A row that already fills the terminal's width has nowhere to shift to, so a label inserted
    /// at or near its end is pushed off the edge. Use [`Self::overlay`] to anchor a label to a
    /// column instead.
    pub fn label(row: usize, col: usize, span: Span) -> Self {
        Self {
            row,
            start_col: col,
            end_col: col,
            style: Style::default(),
            text: Some(DecorationText::Insert(span)),
        }
    }

    /// Paint a styled span over the columns starting at `col` on `row`, covering what is there.
    ///
    /// Column positions are preserved, so the label stays on the content it marks and a fixed-width
    /// row keeps its width.
    pub fn overlay(row: usize, col: usize, span: Span) -> Self {
        Self {
            row,
            start_col: col,
            end_col: col,
            style: Style::default(),
            text: Some(DecorationText::Overlay(span)),
        }
    }
}

impl Default for TerminalRenderSnapshot {
    fn default() -> Self {
        Self {
            text: Arc::from(""),
            color_lines: Arc::new([vec![Span::new("")]]),
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            cursor_shape: CaretShape::Block,
            cursor_blinking: true,
            sequence: 0,
            scrollback_offset: 0,
            total_scrollback_rows: 0,
            evicted_lines: 0,
            history_epoch: 0,
            mouse_mode: MouseModeState::default(),
            key_modes: TerminalKeyModes::default(),
            #[cfg(feature = "terminal-images")]
            images: Arc::from([]),
            wrapped_rows: Arc::from([]),
        }
    }
}

impl TerminalRenderSnapshot {
    /// Build a render snapshot from owned parts.
    ///
    /// This constructor is intended for external render-snapshot transports that
    /// keep their own versioned wire format. It does not make
    /// `TerminalRenderSnapshot` itself a stable wire protocol.
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        text: impl Into<Arc<str>>,
        color_lines: Vec<Vec<Span>>,
        cursor_row: u16,
        cursor_col: u16,
        cursor_visible: bool,
        cursor_shape: CaretShape,
        cursor_blinking: bool,
        sequence: u64,
        scrollback_offset: usize,
        total_scrollback_rows: usize,
        mouse_mode: MouseModeState,
        key_modes: TerminalKeyModes,
    ) -> Self {
        Self::from_parts_inner(
            text,
            color_lines,
            cursor_row,
            cursor_col,
            cursor_visible,
            cursor_shape,
            cursor_blinking,
            sequence,
            scrollback_offset,
            total_scrollback_rows,
            0,
            0,
            mouse_mode,
            key_modes,
        )
    }

    /// Attach per-row soft-wrap flags to an externally built snapshot.
    ///
    /// `wrapped_rows[row]` says row `row` continues into `row + 1`. A transport that cannot carry
    /// the flags simply omits this, and callers see a viewport where nothing wraps.
    pub fn with_wrapped_rows(mut self, wrapped_rows: impl Into<Arc<[bool]>>) -> Self {
        self.wrapped_rows = wrapped_rows.into();
        self
    }

    /// Attach scrollback lineage counters to an externally built snapshot.
    pub fn with_scrollback_lineage(mut self, evicted_lines: u64, history_epoch: u64) -> Self {
        self.evicted_lines = evicted_lines;
        self.history_epoch = history_epoch;
        self
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts_inner(
        text: impl Into<Arc<str>>,
        color_lines: Vec<Vec<Span>>,
        cursor_row: u16,
        cursor_col: u16,
        cursor_visible: bool,
        cursor_shape: CaretShape,
        cursor_blinking: bool,
        sequence: u64,
        scrollback_offset: usize,
        total_scrollback_rows: usize,
        evicted_lines: u64,
        history_epoch: u64,
        mouse_mode: MouseModeState,
        key_modes: TerminalKeyModes,
    ) -> Self {
        Self {
            text: text.into(),
            color_lines: Arc::from(color_lines.into_boxed_slice()),
            cursor_row,
            cursor_col,
            cursor_visible,
            cursor_shape,
            cursor_blinking,
            sequence,
            scrollback_offset,
            total_scrollback_rows,
            evicted_lines,
            history_epoch,
            mouse_mode,
            key_modes,
            #[cfg(feature = "terminal-images")]
            images: Arc::from([]),
            wrapped_rows: Arc::from([]),
        }
    }

    /// Extract a selection from the styled visible grid using display columns.
    ///
    /// This deliberately reads [`Self::color_lines`] rather than [`Self::text`], because the
    /// latter has no display-column mapping once wide characters are present. Call it on the
    /// undecorated snapshot when labels or other render-only overlays must not be copied.
    pub fn selection_text(
        &self,
        selection: &GridSelection,
        endpoint: SelectionEnd,
        trim_row_end: bool,
    ) -> String {
        terminal_selection_text_with(&self.color_lines, selection, endpoint, trim_row_end)
    }

    /// Return a copy of this snapshot with display-column decorations applied to its styled lines.
    ///
    /// Decorations are grouped and restyled once per row, then inserted labels are applied from
    /// right to left so earlier display columns remain stable. The plain [`Self::text`] is left
    /// unchanged by design, keeping render-only overlays out of plain-text scanners. Copy from
    /// the undecorated snapshot when labels should not be included. The sequence combines the
    /// source sequence with an order-sensitive decoration hash.
    pub fn decorated(&self, decorations: &[TerminalDecoration]) -> Self {
        if decorations.is_empty() {
            return self.clone();
        }

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.sequence.hash(&mut hasher);
        decorations.hash(&mut hasher);
        let sequence = hasher.finish();

        let mut rows: BTreeMap<usize, Vec<(&TerminalDecoration, usize)>> = BTreeMap::new();
        for (index, decoration) in decorations.iter().enumerate() {
            rows.entry(decoration.row)
                .or_default()
                .push((decoration, index));
        }

        let mut color_lines: Vec<Vec<Span>> = self.color_lines.iter().cloned().collect();
        for (row, decorations) in rows {
            let Some(line) = color_lines.get_mut(row) else {
                continue;
            };
            let mut sorted = decorations;
            sorted.sort_by_key(|(decoration, index)| (decoration.start_col, *index));

            let ranges: Vec<_> = sorted
                .iter()
                .filter(|(decoration, _)| decoration.text.is_none())
                .map(|(decoration, _)| (decoration.start_col..decoration.end_col, decoration.style))
                .collect();
            if !ranges.is_empty() {
                *line = crate::utils::spans::restyle_columns(line, &ranges);
            }

            // Overlays first, and left to right: they neither move nor read the columns an insert
            // would shift, so doing them before the inserts keeps their anchors the plain ones the
            // caller passed. Inserts then run right to left as before.
            for (decoration, _) in &sorted {
                if let Some(DecorationText::Overlay(overlay)) = decoration.text.clone() {
                    *line = crate::utils::spans::overwrite_at_column(
                        line,
                        decoration.start_col,
                        overlay,
                    );
                }
            }
            for (decoration, _) in sorted.iter().rev() {
                if let Some(DecorationText::Insert(insert)) = decoration.text.clone() {
                    *line =
                        crate::utils::spans::insert_at_column(line, decoration.start_col, insert);
                }
            }
        }

        Self {
            color_lines: color_lines.into(),
            sequence,
            ..self.clone()
        }
    }
}

/// Color palette used to resolve terminal ANSI/default colors into concrete UI colors.
///
/// This affects render snapshots produced by [`TerminalScreen`]. Truecolor escape
/// sequences are preserved as-is; 16-color ANSI slots and default foreground/background
/// colors are resolved through this palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalColorPalette {
    /// Terminal default foreground (`SGR 39`, [`NamedColor::Foreground`]).
    pub foreground: Option<UiColor>,
    /// Terminal default background (`SGR 49`, [`NamedColor::Background`]).
    pub background: Option<UiColor>,
    /// ANSI slots 0..15: black, red, green, yellow, blue, magenta, cyan, white,
    /// then bright black through bright white.
    pub ansi: [UiColor; 16],
}

impl Default for TerminalColorPalette {
    fn default() -> Self {
        Self {
            foreground: None,
            background: None,
            ansi: default_ansi_palette(),
        }
    }
}

impl TerminalColorPalette {
    /// Create a palette from default foreground/background colors and 16 ANSI slots.
    pub fn new(foreground: UiColor, background: UiColor, ansi: [UiColor; 16]) -> Self {
        Self {
            foreground: Some(foreground),
            background: Some(background),
            ansi,
        }
    }

    /// Create a terminal palette from a probed host terminal palette.
    ///
    /// The host default foreground and ANSI 0..15 slots are preserved exactly, while
    /// `background` becomes the emulated terminal's default background. This lets
    /// apps keep ANSI colors faithful to the real terminal while still choosing an
    /// app-owned surface color for embedded terminal panes.
    pub fn from_host_colors(colors: HostTerminalColors, background: UiColor) -> Self {
        Self::new(colors.fg, background, colors.ansi)
    }

    /// Create a terminal palette from an application theme.
    ///
    /// A [`HostTerminalColors`] theme extension takes precedence so a probed ANSI palette is
    /// preserved exactly. Otherwise the palette is derived from the theme's semantic status,
    /// icon, accent, and muted colors. `background` is resolved to black when it is a sentinel,
    /// matching terminal protocol defaults.
    pub fn from_theme(theme: &Theme, background: UiColor) -> Self {
        let resolve_style_fg = |style: Style, fallback: UiColor| {
            style
                .resolved_fg()
                .map(|color| color.resolve(UiColor::Reset))
                .filter(|color| !color.is_sentinel())
                .unwrap_or(fallback)
        };
        let foreground = resolve_style_fg(theme.primary, UiColor::White);
        let background = background.resolve(UiColor::Black);
        if let Some(host_colors) = theme.extension::<HostTerminalColors>() {
            return Self::from_host_colors(*host_colors, background);
        }

        let muted = resolve_style_fg(theme.muted, theme.surface.menu.resolve(background));
        let accent = resolve_style_fg(theme.accent, theme.border_active.resolve(foreground));
        let error = theme.status.error.resolve(UiColor::Red);
        let success = theme.status.success.resolve(UiColor::Green);
        let warning = theme.status.warning.resolve(UiColor::Yellow);
        let info = theme.status.info.resolve(accent);
        let purple = theme.file_icons.purple.resolve(UiColor::Magenta);
        let cyan = theme.file_icons.cyan.resolve(UiColor::Cyan);

        Self::new(
            foreground,
            background,
            [
                background,
                error,
                success,
                warning,
                info,
                purple,
                cyan,
                foreground,
                muted,
                error.lighten_by(0.18),
                success.lighten_by(0.18),
                warning.lighten_by(0.18),
                accent.lighten_by(0.12),
                purple.lighten_by(0.18),
                cyan.lighten_by(0.18),
                foreground.lighten_by(0.12),
            ],
        )
    }

    /// Set the terminal default foreground color.
    pub fn foreground(mut self, color: Option<UiColor>) -> Self {
        self.foreground = color;
        self
    }

    /// Set the terminal default background color.
    pub fn background(mut self, color: Option<UiColor>) -> Self {
        self.background = color;
        self
    }

    /// Set all 16 ANSI color slots.
    pub fn ansi(mut self, ansi: [UiColor; 16]) -> Self {
        self.ansi = ansi;
        self
    }
}

impl TerminalScreen {
    /// Create an Alacritty terminal-backed screen with bounded scrollback.
    pub fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let dimensions = TermDimensions {
            rows: rows as usize,
            cols: cols as usize,
        };
        // Extra headroom above the exposed scrollback so the grid can never saturate
        // inside a single handler call; `LedgerTerm` trims back down to `scrollback`
        // and counts what it dropped. See `scrollback_ledger`.
        let capacity = ledger_capacity(scrollback, rows);
        let config = TermConfig {
            scrolling_history: capacity,
            default_cursor_style: DEFAULT_CURSOR_STYLE,
            // Track Kitty keyboard protocol pushes so `key_modes()` can report what the child
            // negotiated; without this alacritty silently drops every `CSI > <flags> u`.
            kitty_keyboard: true,
            ..TermConfig::default()
        };
        let listener = ResponseCapture::default();
        let term = Term::new(config, &dimensions, listener.clone());
        let screen = Self {
            processor: VteProcessor::new(),
            term,
            listener,
            semantic_parser: SemanticVteParser::new(),
            semantic: SemanticObserver::default(),
            rows,
            cols,
            scrollback_len: scrollback,
            ledger_capacity: capacity,
            mouse_mode: MouseModeState::default(),
            pixel_mouse: false,
            scrollback_offset: 0,
            cache: TerminalRenderSnapshot::default(),
            palette: TerminalColorPalette::default(),
            dirty: true,
            sequence: 0,
            semantic_marks: VecDeque::new(),
            semantic_events_seen: 0,
            prompt_boundary_scanner: PromptBoundaryScanner::default(),
            active_prompt_mark: None,
            #[cfg(feature = "terminal-images")]
            graphics_scanner: GraphicsScanner::default(),
            #[cfg(feature = "terminal-images")]
            graphics: TerminalGraphics::default(),
            #[cfg(feature = "terminal-images")]
            graphics_alt_screen: false,
            evicted_lines: 0,
            history_epoch: 0,
            alt_screen: false,
            cell_size: TerminalCellSize::default(),
        };
        screen.sync_viewport();
        screen
    }

    /// Feed terminal bytes.
    ///
    pub fn process_bytes(&mut self, bytes: &[u8]) {
        let evicted = self.feed_grid_with_prompt_boundaries(bytes);
        if evicted > 0 {
            self.evicted_lines = self.evicted_lines.saturating_add(evicted as u64);
        }
        self.semantic_parser.advance(&mut self.semantic, bytes);
        self.settle_graphics(evicted);
        let alt_screen = self.term.mode().contains(TermMode::ALT_SCREEN);
        if self.alt_screen != alt_screen {
            self.history_epoch = self.history_epoch.saturating_add(1);
            self.alt_screen = alt_screen;
        }
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            // Alt-screen programs still emit OSC 133, but those marks belong to a grid
            // with no scrollback and no absolute-line space of its own. Discard them
            // rather than leaving them pending, or they get replayed against
            // main-screen coordinates the moment the alt screen is torn down.
            self.discard_pending_semantic_marks();
            self.active_prompt_mark = None;
        } else {
            self.record_semantic_marks_from_pending();
        }
        self.scrollback_offset = self.term.grid().display_offset();
        self.mouse_mode = mouse_mode_from_term(*self.term.mode(), self.pixel_mouse);
        self.dirty = true;
    }

    fn feed_grid_with_prompt_boundaries(&mut self, bytes: &[u8]) -> usize {
        let boundaries = self.prompt_boundary_scanner.scan(bytes);
        let mut evicted_total = 0;
        let mut start = 0;
        for (end, boundary) in boundaries {
            let evicted = self.feed_grid(&bytes[start..end]);
            self.drop_evicted_semantic_marks(evicted);
            evicted_total += evicted;
            self.apply_prompt_boundary(boundary);
            start = end;
        }

        let evicted = self.feed_grid(&bytes[start..]);
        self.drop_evicted_semantic_marks(evicted);
        evicted_total + evicted
    }

    fn apply_prompt_boundary(&mut self, boundary: PromptBoundary) {
        match boundary {
            PromptBoundary::Start if !self.term.mode().contains(TermMode::ALT_SCREEN) => {
                self.active_prompt_mark = Some(ActivePromptMark {
                    absolute_line: self.cursor_absolute_line(),
                    column: self.term.grid().cursor.point.column.0,
                });
            }
            PromptBoundary::Start | PromptBoundary::CommandStart => {
                self.active_prompt_mark = None;
            }
        }
    }

    /// Drive the grid parser, returning how many scrollback lines the chunk evicted.
    ///
    /// With image support compiled in, graphics commands are lifted out of the stream first and
    /// the grid sees only what is left, plus whatever cursor movement each placement implies. The
    /// VT parser discards `APC` bodies anyway, so removing them changes nothing it would have
    /// done - what it buys is the cursor position *at* each command, which a parser running
    /// alongside this one could not observe.
    #[cfg(feature = "terminal-images")]
    fn feed_grid(&mut self, bytes: &[u8]) -> usize {
        if self.graphics_scanner.is_plain(bytes) {
            return self.advance_vte(bytes);
        }

        let mut evicted = 0;
        for segment in self.graphics_scanner.scan(bytes) {
            evicted += match segment {
                GraphicsSegment::Text(range) => self.advance_vte(&bytes[range]),
                GraphicsSegment::HeldEscape => self.advance_vte(&[0x1b]),
                GraphicsSegment::Command(command) => self.apply_graphics(*command),
            };
        }
        evicted
    }

    #[cfg(not(feature = "terminal-images"))]
    fn feed_grid(&mut self, bytes: &[u8]) -> usize {
        self.advance_vte(bytes)
    }

    fn advance_vte(&mut self, bytes: &[u8]) -> usize {
        if bytes.is_empty() {
            return 0;
        }
        let mut ledger = LedgerTerm::new(
            &mut self.term,
            self.scrollback_len,
            self.ledger_capacity,
            HostModes {
                pixel_mouse: &mut self.pixel_mouse,
                responses: &self.listener.responses,
            },
        );
        self.processor.advance(&mut ledger, bytes);
        ledger.evicted()
    }

    /// Run one graphics command against the store, then apply what it implies to the grid.
    #[cfg(feature = "terminal-images")]
    fn apply_graphics(&mut self, command: GraphicsCommand) -> usize {
        let ctx = GraphicsContext {
            cursor_line: self.cursor_absolute_line(),
            cursor_col: u16::try_from(self.term.grid().cursor.point.column.0).unwrap_or(u16::MAX),
            viewport_top_line: self.term.history_size(),
            alt_screen: self.term.mode().contains(TermMode::ALT_SCREEN),
            cell: self.cell_size,
            cols: self.cols,
        };
        let outcome = self.graphics.apply(command, ctx);
        if let Some(response) = outcome.response {
            self.listener.responses.borrow_mut().push(response);
        }
        let Some((rows, cols)) = outcome.advance else {
            return 0;
        };

        // The protocol leaves the cursor just past the image: down by its height minus one, right
        // by its width. Synthesizing that as real output rather than moving the cursor directly is
        // what makes the grid scroll to make room, exactly as it would for the same many lines of
        // text - which in turn is what keeps the placement's absolute-line anchor meaningful.
        let mut movement = vec![b'\n'; usize::from(rows.saturating_sub(1))];
        movement.extend_from_slice(format!("\x1b[{cols}C").as_bytes());
        self.advance_vte(&movement)
    }

    /// Every image overlapping the viewport: those placed at a cursor, and those the grid names
    /// with placeholder cells.
    ///
    /// Both kinds are wanted at once - a session can have `icat` output scrolled up the pane while
    /// a TUI below it draws with placeholders - so the two lists are simply concatenated and left
    /// in back-to-front order.
    #[cfg(feature = "terminal-images")]
    fn visible_images(
        &self,
        display_offset: usize,
        alt_screen: bool,
    ) -> Vec<TerminalImagePlacement> {
        let mut images = self.graphics.visible(
            self.term.history_size(),
            display_offset,
            self.rows,
            alt_screen,
        );
        images.extend(
            self.graphics
                .placeholder_placements(&self.placeholder_cells(display_offset), self.cell_size),
        );
        images
    }

    /// Read the viewport's placeholder cells, left to right and top to bottom.
    ///
    /// Walks the grid directly rather than the render iterator: the marks that carry a cell's
    /// position inside its image are zero-width characters, which the styled-span path folds into
    /// text and cannot be recovered from.
    #[cfg(feature = "terminal-images")]
    fn placeholder_cells(&self, display_offset: usize) -> Vec<PlaceholderCell> {
        let mut cells = Vec::new();
        // Nothing can name an image that was never transmitted, so a session that has seen no
        // graphics never pays for this walk.
        if !self.graphics.has_images() {
            return cells;
        }

        let grid = self.term.grid();
        for row in 0..self.rows {
            let line = Line(i32::from(row) - display_offset as i32);
            if line < grid.topmost_line() || line > grid.bottommost_line() {
                continue;
            }
            for col in 0..self.cols {
                let cell = &grid[line][Column(col as usize)];
                if cell.c != PLACEHOLDER {
                    continue;
                }
                let Some(id_low) = placeholder_id(cell) else {
                    continue;
                };
                cells.push(PlaceholderCell::new(
                    row,
                    col,
                    id_low,
                    cell.zerowidth().unwrap_or(&[]),
                ));
            }
        }
        cells
    }

    /// Bring image placements back in line with the grid after a chunk.
    #[cfg(feature = "terminal-images")]
    fn settle_graphics(&mut self, evicted: usize) {
        self.graphics.drop_evicted(evicted);
        let alt_screen = self.term.mode().contains(TermMode::ALT_SCREEN);
        if self.graphics_alt_screen && !alt_screen {
            // The alternate screen is gone, and so is everything drawn on it.
            self.graphics.clear_alt_screen();
        }
        self.graphics_alt_screen = alt_screen;
    }

    #[cfg(not(feature = "terminal-images"))]
    fn settle_graphics(&mut self, _evicted: usize) {}

    /// Set the host's cell size in pixels.
    ///
    /// This is what `CSI 14 t` reports and, with the `terminal-images` feature, what decides how
    /// many cells an image covers. Install the same value the child is told through the PTY (see
    /// [`TerminalPty::resize_with_cell_size`](super::TerminalPty::resize_with_cell_size)), so a
    /// program that sizes a picture for itself and this screen agree; a mismatch shows up as
    /// images that overlap the text below them or leave a gap.
    pub fn set_cell_size(&mut self, cell: TerminalCellSize) {
        if self.cell_size != cell {
            self.cell_size = cell;
            self.sync_viewport();
            self.dirty = true;
        }
    }

    /// The cell size reported to the child.
    pub fn cell_size(&self) -> TerminalCellSize {
        self.cell_size
    }

    /// Whether resizing to `new_cols` would move text between lines.
    ///
    /// Only two things can: a line already wrapped into the next one, which widening would pull
    /// back up, and a line reaching past the new width, which narrowing would push down. With
    /// neither present every line keeps exactly the text it has, and so does anything anchored to
    /// it. Worth the scan - the alternative is treating every width change as a rewrap, which in a
    /// tiling multiplexer costs a pane every image in it each time a neighbour opens.
    #[cfg(feature = "terminal-images")]
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
                if cell.c != ' ' || cell.bg != TermColor::Named(NamedColor::Background) {
                    return true;
                }
            }
        }
        false
    }

    /// Republish the viewport to the listener that answers pixel-size queries.
    fn sync_viewport(&self) {
        self.listener.viewport.set(ViewportGeometry {
            rows: self.rows,
            cols: self.cols,
            cell: self.cell_size,
        });
    }

    /// Cap the decoded pixels this screen retains, in bytes.
    ///
    /// Images past the cap are dropped least-recently-used, placements included. The default is
    /// 96 MiB, which is roughly sixteen 1080p frames.
    #[cfg(feature = "terminal-images")]
    pub fn set_image_budget(&mut self, bytes: usize) {
        self.graphics.set_budget(bytes);
        self.dirty = true;
    }

    /// Choose whether this screen retains and decodes terminal image pixels.
    ///
    /// Disabling storage keeps graphics protocol replies and cursor movement correct while
    /// retaining only image dimensions. It is intended for server-side semantic mirrors that
    /// forward the original byte stream to a separate rendering client.
    #[cfg(feature = "terminal-images")]
    pub fn set_image_storage_enabled(&mut self, enabled: bool) {
        self.graphics_scanner.set_decode_payload(enabled);
        self.graphics.set_storage_enabled(enabled);
        self.dirty = true;
    }

    /// Choose which out-of-band transmission media (`t=f`, `t=t`, `t=s`) this screen accepts.
    ///
    /// These let a child leave a frame in a file or a shared-memory object and name it in a
    /// hundred-byte escape, instead of compressing and base64-ing megabytes of pixels through the
    /// PTY every frame. `t=t` and `t=s` are claimed by being read, so anything that fans one
    /// terminal stream out to several readers wants
    /// [`GraphicsMediaPolicy::SHARED`](crate::widgets::GraphicsMediaPolicy::SHARED), which keeps only
    /// the re-readable `t=f`.
    #[cfg(feature = "terminal-images")]
    pub fn set_image_media_policy(&mut self, media: super::graphics_media::GraphicsMediaPolicy) {
        self.graphics.set_media_policy(media);
    }

    /// Whether this screen currently retains any Kitty graphics.
    ///
    /// True after a child has transmitted at least one image that has not been deleted or
    /// evicted. Visible placements this frame are [`TerminalRenderSnapshot::images`]; this
    /// flag stays set while those pixels are still in the store, including when they have
    /// scrolled out of the viewport. That is the class of panes whose host-side image layer
    /// does not follow a widget shrink or fade.
    #[cfg(feature = "terminal-images")]
    pub fn has_images(&self) -> bool {
        self.graphics.has_images()
    }

    /// Return the current working-directory/command-lifecycle state accumulated from `OSC
    /// 7`/`OSC 9;9`/`OSC 133` sequences seen so far.
    ///
    /// This is runtime metadata, not render state: it is never part of
    /// [`TerminalRenderSnapshot`] and does not participate in `dirty`/cache invalidation.
    pub fn semantic_state(&self) -> TerminalSemanticState {
        self.semantic.state()
    }

    /// Drain semantic-state changes observed since the last call.
    ///
    /// Call this after [`process_bytes`](Self::process_bytes) alongside
    /// [`drain_responses`](Self::drain_responses) to react to CWD/command-phase/executable
    /// changes without re-deriving them from [`semantic_state`](Self::semantic_state) on every
    /// poll.
    pub fn drain_semantic_events(&mut self) -> Vec<TerminalSemanticEvent> {
        self.semantic_events_seen = 0;
        self.semantic.drain_events()
    }

    /// Current scrollback lineage counters for selection rebasing.
    pub fn scrollback_lineage(&self) -> ScrollbackLineage {
        ScrollbackLineage {
            evicted_lines: self.evicted_lines,
            history_epoch: self.history_epoch,
        }
    }

    /// Export an absolute selection using display columns across retained scrollback lines.
    pub fn selection_display_text(
        &self,
        sel: &TerminalSelection,
        endpoint: SelectionEnd,
        trim_row_end: bool,
    ) -> String {
        if sel.is_empty() && matches!(endpoint, SelectionEnd::Exclusive) {
            return String::new();
        }

        let (start, end) = sel.normalized();
        let total = self.total_text_lines();
        let row_start = start.line.min(total);
        let row_end = end.line.min(total.saturating_sub(1));
        if row_start > row_end {
            return String::new();
        }

        let grid = self.term.grid();
        let top = grid.topmost_line().0;
        let mut result = String::new();
        for absolute in row_start..=row_end {
            let line = Line(top + absolute as i32);
            let col_start = if absolute == start.line { start.col } else { 0 };
            let col_end = if absolute == end.line {
                end.col
                    .saturating_add(matches!(endpoint, SelectionEnd::Inclusive) as usize)
            } else {
                display_line_width(grid, line)
            };
            let mut text = display_columns_text(grid, line, col_start, col_end);
            if trim_row_end {
                text.truncate(text.trim_end().len());
            }
            result.push_str(&text);
            if absolute < row_end {
                result.push('\n');
            }
        }
        result
    }

    /// Reapply previously captured semantic state without replaying escape sequences.
    ///
    /// Intended for restoring state across a fresh `TerminalScreen` (e.g. session
    /// resurrection/reattach) where the byte stream that originally produced it is not being
    /// replayed. Does not emit [`TerminalSemanticEvent`]s - the caller already knows the state
    /// it is installing.
    pub fn restore_semantic_state(&mut self, state: TerminalSemanticState) {
        self.semantic.restore_state(state);
    }

    /// Drain and return any PTY responses that need to be written back.
    ///
    /// Call this after `process_bytes()` to get responses like device attribute
    /// queries, cursor position reports, etc. These should be written back to
    /// the PTY stdin.
    pub fn drain_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.listener.responses.borrow_mut())
    }

    /// Drain decoded OSC 52 clipboard-store requests emitted by the child.
    ///
    /// Applying these requests is deliberately host policy: an application can ignore them,
    /// write them to a local clipboard, or relay them through an outer terminal. OSC 52 clipboard
    /// loads remain disabled, so a child cannot read clipboard contents through this parser.
    pub fn drain_clipboard_events(&mut self) -> Vec<TerminalClipboardEvent> {
        std::mem::take(&mut *self.listener.clipboard_events.borrow_mut())
    }

    /// Return the number of BEL events received since this screen was created.
    pub fn bell_count(&self) -> u64 {
        self.listener.bell_count.get()
    }

    /// Serialize the current terminal state as bytes that can be replayed by a
    /// fresh same-sized [`TerminalScreen`].
    ///
    /// The stream captures scrollback, primary/alternate screen contents, the
    /// current cursor position/template, title, and common terminal modes. It is
    /// intentionally a replay stream rather than a stable data format: replaying
    /// it goes through the normal VTE parser and future parser fixes naturally
    /// apply to exported state.
    ///
    /// Non-goals: tab stops, custom scrolling regions, cursor style, kitty
    /// keyboard stack depth (the effective flags are preserved), hyperlinks,
    /// and the current display offset. The receiver lands on the live view.
    pub fn export_replay_bytes(&mut self) -> Vec<u8> {
        let dirty = self.dirty;
        let cache = self.cache.clone();
        let sequence = self.sequence;
        let scrollback_offset = self.scrollback_offset;
        let mouse_mode = self.mouse_mode;
        let responses = self.drain_responses();

        let was_alt = self.term.mode().contains(TermMode::ALT_SCREEN);
        let bytes = if was_alt {
            let saved_alt_cursor = self.term.grid().cursor.clone();
            let saved_alt_saved_cursor = self.term.grid().saved_cursor.clone();
            let alt_repaint = self.export_active_grid_repaint(false);
            self.term.swap_alt();
            let mut bytes = self.export_primary_replay();

            // Switching primary -> alt clears the alt grid, so immediately
            // replay the synthesized alt repaint to restore the source screen.
            self.term.swap_alt();
            let mut repair_processor: VteProcessor = VteProcessor::new();
            repair_processor.advance(&mut self.term, &alt_repaint);
            self.term.grid_mut().cursor = saved_alt_cursor;
            self.term.grid_mut().saved_cursor = saved_alt_saved_cursor;

            bytes.extend_from_slice(b"\x1b[?1049h");
            bytes.extend_from_slice(&alt_repaint);
            self.push_cursor_position(&mut bytes);
            self.push_modes(&mut bytes);
            bytes
        } else {
            self.export_primary_replay()
        };

        *self.listener.responses.borrow_mut() = responses;
        self.dirty = dirty;
        self.cache = cache;
        self.sequence = sequence;
        self.scrollback_offset = scrollback_offset;
        self.mouse_mode = mouse_mode;
        bytes
    }

    /// Resize screen dimensions.
    ///
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let dimensions_changed = rows != self.rows || cols != self.cols;
        let reflowed = cols != self.cols;
        if dimensions_changed {
            self.clear_active_prompt();
        }
        self.rows = rows;
        self.cols = cols;
        let dimensions = TermDimensions {
            rows: self.rows as usize,
            cols: self.cols as usize,
        };
        #[cfg(feature = "terminal-images")]
        let rewraps = self.width_change_rewraps(cols.max(1));
        self.term.resize(dimensions);
        self.sync_viewport();
        // `Term::resize` can push lines into history without going through a handler
        // call, so the ledger never sees it; trim and account for it here.
        self.ledger_capacity = ledger_capacity(self.scrollback_len, self.rows);
        let evicted = settle_history(&mut self.term, self.scrollback_len, self.ledger_capacity);
        if evicted > 0 {
            self.evicted_lines = self.evicted_lines.saturating_add(evicted as u64);
        }
        if reflowed {
            self.history_epoch = self.history_epoch.saturating_add(1);
            // A column change rewraps history, so line indices no longer refer to the
            // text they were recorded against and cannot be corrected by a shift.
            self.semantic_marks.clear();
            // Images are dropped on the same reasoning, but only when the rewrap actually
            // happened. Treating every width change as a rewrap costs every image in the pane on
            // every resize, which in a tiling multiplexer is every split - and a pane full of
            // plots going blank because a neighbour opened is worse than the drift this risks.
            #[cfg(feature = "terminal-images")]
            if rewraps {
                self.graphics.clear_placements();
            }
        } else {
            self.drop_evicted_semantic_marks(evicted);
            self.settle_graphics(evicted);
        }
        let alt_screen = self.term.mode().contains(TermMode::ALT_SCREEN);
        if self.alt_screen != alt_screen {
            self.history_epoch = self.history_epoch.saturating_add(1);
            self.alt_screen = alt_screen;
        }
        self.scrollback_offset = self.term.grid().display_offset();
        self.mouse_mode = mouse_mode_from_term(*self.term.mode(), self.pixel_mouse);
        self.dirty = true;
    }

    /// Erase an active semantic prompt before grid reflow.
    ///
    /// Shells receive `SIGWINCH` after a PTY resize and redraw their current prompt. Leaving the
    /// old prompt in the reflow buffer makes that redraw additive, duplicating wrapped input.
    fn clear_active_prompt(&mut self) {
        let Some(mark) = self.active_prompt_mark.take() else {
            return;
        };
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }

        let grid = self.term.grid_mut();
        let line = grid.topmost_line().0 + mark.absolute_line as i32;
        let screen_lines = grid.screen_lines() as i32;
        if !(0..screen_lines).contains(&line) {
            return;
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
    }

    /// Return current visible screen contents.
    pub fn snapshot(&mut self) -> Arc<str> {
        self.render_snapshot().text
    }

    /// Return full render snapshot (text, colors, cursor).
    pub fn render_snapshot(&mut self) -> TerminalRenderSnapshot {
        if self.dirty {
            self.sequence = self.sequence.saturating_add(1);
            let content = self.term.renderable_content();
            let display_offset = content.display_offset;
            let mode = content.mode;
            let cursor = content.cursor;
            let display_iter = content.display_iter;
            self.scrollback_offset = display_offset;
            self.mouse_mode = mouse_mode_from_term(mode, self.pixel_mouse);

            let cursor_view = term::point_to_viewport(display_offset, cursor.point);
            let cursor_row = cursor_view.as_ref().map(|p| p.line as u16).unwrap_or(0);
            let cursor_col = cursor_view.as_ref().map(|p| p.column.0 as u16).unwrap_or(0);
            let cursor_visible =
                mode.contains(TermMode::SHOW_CURSOR) && self.scrollback_offset == 0;
            let cursor_style = self.term.cursor_style();
            let cursor_shape = caret_shape_from_term(cursor_style.shape);
            let cursor_blinking = cursor_style.blinking;

            let mut visible = renderable_content_lines(
                display_iter,
                display_offset,
                self.rows,
                self.cols,
                self.palette,
            );
            if visible.is_empty() {
                visible.push(vec![Span::new("")]);
            }

            #[cfg(feature = "terminal-images")]
            let images = self.visible_images(display_offset, mode.contains(TermMode::ALT_SCREEN));

            let wrapped_rows = visible_wrapped_rows(self.term.grid(), display_offset, self.rows);

            let mut text = String::new();
            for (idx, line) in visible.iter().enumerate() {
                if idx > 0 {
                    text.push('\n');
                }
                for span in line {
                    text.push_str(span.content.as_ref());
                }
            }

            self.cache = TerminalRenderSnapshot {
                text: Arc::from(text),
                color_lines: visible.into(),
                cursor_row,
                cursor_col,
                cursor_visible,
                cursor_shape,
                cursor_blinking,
                sequence: self.sequence,
                scrollback_offset: self.scrollback_offset,
                total_scrollback_rows: self.term.history_size(),
                evicted_lines: self.evicted_lines,
                history_epoch: self.history_epoch,
                mouse_mode: self.mouse_mode,
                key_modes: key_modes_from_term(mode),
                #[cfg(feature = "terminal-images")]
                images: images.into(),
                wrapped_rows: wrapped_rows.into(),
            };
            self.dirty = false;
        }
        self.cache.clone()
    }

    /// Return the current terminal color palette.
    pub fn palette(&self) -> TerminalColorPalette {
        self.palette
    }

    /// Set the terminal color palette used for future render snapshots.
    pub fn set_palette(&mut self, palette: TerminalColorPalette) {
        if self.palette != palette {
            self.palette = palette;
            // Keep the listener's copy in sync so `OSC 4/10/11 ; ?` color
            // queries are answered against the current palette.
            *self.listener.palette.borrow_mut() = palette;
            self.dirty = true;
        }
    }

    /// Return the current scrollback offset (0 = live view).
    pub fn scrollback_offset(&self) -> usize {
        self.scrollback_offset
    }

    /// Set the scrollback viewing offset.
    ///
    /// 0 = live view (bottom of scrollback), positive values scroll into
    /// history. The value is clamped to the actual scrollback size.
    pub fn set_scrollback(&mut self, offset: usize) {
        let max_offset = self.term.history_size();
        let target = offset.min(max_offset);
        let current = self.term.grid().display_offset();
        let delta = target as i32 - current as i32;
        if delta != 0 {
            self.term.scroll_display(Scroll::Delta(delta));
        }
        self.scrollback_offset = self.term.grid().display_offset();
        self.dirty = true;
    }

    /// Probe total scrollback rows available.
    pub fn total_scrollback_rows(&mut self) -> usize {
        self.term.history_size()
    }

    /// Number of plain-text lines currently retained (scrollback history + visible screen).
    ///
    /// Absolute line indices used by [`text_lines`](Self::text_lines) /
    /// [`export_text`](Self::export_text) count from the oldest retained history line (`0`)
    /// through the live bottom (`total_text_lines() - 1`).
    pub fn total_text_lines(&self) -> usize {
        let grid = self.term.grid();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        usize::try_from(bottom.saturating_sub(top).saturating_add(1)).unwrap_or(0)
    }

    /// Visit plain-text grid lines in `[start, end)`, addressed from the oldest retained line.
    ///
    /// Bounds are clamped to the retained line count. Each visit receives its absolute retained-line
    /// index and text. The same scratch allocation is reused for every line, so the `&str` passed
    /// to `visitor` is valid only for that callback invocation. Returning [`ControlFlow::Break`]
    /// stops before any later line is extracted.
    pub fn try_for_each_text_line(
        &self,
        start: usize,
        end: usize,
        mut visitor: impl FnMut(usize, &str) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        let total = self.total_text_lines();
        let start = start.min(total);
        let end = end.min(total).max(start);
        let grid = self.term.grid();
        let top = grid.topmost_line().0;
        let mut scratch = String::with_capacity(grid.columns());

        for absolute in start..end {
            scratch.clear();
            push_plain_line_text(grid, Line(top + absolute as i32), &mut scratch);
            visitor(absolute, &scratch)?;
        }
        ControlFlow::Continue(())
    }

    /// Plain text of grid lines in `[start, end)`, addressed from the oldest retained line.
    ///
    /// Does not mutate display offset or go through the render pipeline. Out-of-range bounds
    /// are clamped; empty ranges yield an empty vec.
    pub fn text_lines(&self, start: usize, end: usize) -> Vec<String> {
        let total = self.total_text_lines();
        let start = start.min(total);
        let end = end.min(total).max(start);
        let mut lines = Vec::with_capacity(end - start);
        let _ = self.try_for_each_text_line(start, end, |_, line| {
            lines.push(line.to_owned());
            ControlFlow::Continue(())
        });
        lines
    }

    /// Newline-joined plain text for an absolute line range. See [`text_lines`](Self::text_lines).
    pub fn export_text(&self, start: usize, end: usize) -> String {
        let mut text = String::new();
        let mut first = true;
        let _ = self.try_for_each_text_line(start, end, |_, line| {
            if !first {
                text.push('\n');
            }
            first = false;
            text.push_str(line);
            ControlFlow::Continue(())
        });
        text
    }

    /// Export a selection across retained scrollback lines.
    ///
    /// This path is intentionally **character-indexed** and uses the pre-trimmed text returned by
    /// [`text_lines`](Self::text_lines). It is separate from the display-column snapshot path:
    /// changing `push_plain_line_text` here would alter [`Self::export_text`] and semantic output
    /// exports. With [`SelectionEnd::Inclusive`], both the start and end character positions are
    /// included.
    pub fn export_selection_text(
        &self,
        start: GridPos,
        end: GridPos,
        endpoint: SelectionEnd,
    ) -> String {
        let row_start = start.row.min(end.row);
        let row_end = start.row.max(end.row);
        let selection = GridSelection {
            anchor: GridPos {
                row: start.row - row_start,
                col: start.col,
            },
            cursor: GridPos {
                row: end.row - row_start,
                col: end.col,
            },
        };
        let lines = self.text_lines(row_start, row_end.saturating_add(1));
        selection.extract_text_with(&lines, endpoint, false)
    }

    /// Map an absolute text-line index to `(scrollback_offset, viewport_row)`.
    ///
    /// Returns `None` when the line is outside the currently retained grid (evicted or
    /// out of range). History lines are placed at viewport row 0; live-viewport lines use
    /// offset 0 and their on-screen row.
    pub fn absolute_line_to_viewport(&self, absolute: usize) -> Option<(usize, usize)> {
        let total = self.total_text_lines();
        if absolute >= total {
            return None;
        }
        let grid = self.term.grid();
        let top = grid.topmost_line().0;
        let grid_line = top + absolute as i32;
        if grid_line < 0 {
            let offset = usize::try_from(-grid_line).ok()?;
            Some((offset, 0))
        } else {
            Some((0, grid_line as usize))
        }
    }

    /// Map an absolute text-line index to a scrollback display offset.
    pub fn absolute_line_to_offset(&self, absolute: usize) -> Option<usize> {
        self.absolute_line_to_viewport(absolute)
            .map(|(offset, _)| offset)
    }

    /// Retained OSC 133 marks, oldest first (after eviction GC).
    pub fn semantic_marks(&self) -> Vec<SemanticMark> {
        self.semantic_marks.iter().copied().collect()
    }

    /// Half-open absolute-line range `[start, end)` of the last command's output.
    ///
    /// Uses the last `OutputStart` paired with a following `OutputEnd`. While a command is still
    /// running (start without end), falls back to `[start, total_text_lines())`.
    pub fn last_command_output_range(&self) -> Option<(usize, usize)> {
        let start_idx = self
            .semantic_marks
            .iter()
            .rposition(|mark| mark.kind == SemanticMarkKind::OutputStart)?;
        let start = self.semantic_marks[start_idx].absolute_line;
        let end = self
            .semantic_marks
            .iter()
            .skip(start_idx + 1)
            .find(|mark| mark.kind == SemanticMarkKind::OutputEnd)
            .map(|mark| mark.absolute_line)
            .unwrap_or_else(|| self.total_text_lines());
        Some((start, end.max(start)))
    }

    /// Plain text of [`last_command_output_range`](Self::last_command_output_range), when known.
    pub fn export_last_command_output(&self) -> Option<String> {
        let (start, end) = self.last_command_output_range()?;
        Some(self.export_text(start, end))
    }

    fn cursor_absolute_line(&self) -> usize {
        let grid = self.term.grid();
        let top = grid.topmost_line().0;
        let cursor = grid.cursor.point.line.0;
        usize::try_from((cursor - top).max(0)).unwrap_or(0)
    }

    /// Shift marks down by the lines that just fell out of scrollback, dropping
    /// those whose line is gone.
    ///
    /// `evicted` comes from [`LedgerTerm`], which counts evictions as they happen.
    /// It cannot be re-derived from the grid afterwards: once scrollback is full,
    /// `topmost_line()` and `history_size()` are pinned while content shifts, so a
    /// post-hoc comparison sees nothing and marks silently drift onto unrelated
    /// lines.
    fn drop_evicted_semantic_marks(&mut self, evicted: usize) {
        if evicted == 0 {
            return;
        }
        self.semantic_marks
            .retain(|mark| mark.absolute_line >= evicted);
        for mark in &mut self.semantic_marks {
            mark.absolute_line -= evicted;
        }
        self.active_prompt_mark = self.active_prompt_mark.and_then(|mut mark| {
            if mark.absolute_line < evicted {
                None
            } else {
                mark.absolute_line -= evicted;
                Some(mark)
            }
        });
    }

    /// Consume pending semantic events without recording marks for them.
    fn discard_pending_semantic_marks(&mut self) {
        self.semantic_events_seen = self.semantic.event_count();
    }

    fn record_semantic_marks_from_pending(&mut self) {
        let absolute_line = self.cursor_absolute_line();
        let events = self.semantic.peek_events();
        let from = self.semantic_events_seen.min(events.len());
        let pending: Vec<_> = events[from..].to_vec();
        self.semantic_events_seen = self.semantic.event_count();
        for event in pending {
            let TerminalSemanticEvent::CommandPhaseChanged(phase) = event else {
                continue;
            };
            let mark = match phase {
                TerminalCommandPhase::Prompt => SemanticMark {
                    kind: SemanticMarkKind::Prompt,
                    absolute_line,
                    exit_status: None,
                },
                TerminalCommandPhase::Executing => SemanticMark {
                    kind: SemanticMarkKind::OutputStart,
                    absolute_line,
                    exit_status: None,
                },
                TerminalCommandPhase::Completed { exit_status } => SemanticMark {
                    kind: SemanticMarkKind::OutputEnd,
                    absolute_line,
                    exit_status,
                },
                TerminalCommandPhase::Unknown | TerminalCommandPhase::Input => continue,
            };
            self.semantic_marks.push_back(mark);
            while self.semantic_marks.len() > MAX_SEMANTIC_MARKS {
                self.semantic_marks.pop_front();
            }
        }
    }

    /// Clear parser state and screen.
    pub fn reset(&mut self) {
        let dimensions = TermDimensions {
            rows: self.rows as usize,
            cols: self.cols as usize,
        };
        let config = TermConfig {
            scrolling_history: self.scrollback_len,
            default_cursor_style: DEFAULT_CURSOR_STYLE,
            kitty_keyboard: true,
            ..TermConfig::default()
        };
        self.listener = ResponseCapture::default();
        self.term = Term::new(config, &dimensions, self.listener.clone());
        self.sync_viewport();
        self.processor = VteProcessor::new();
        // Drop any in-flight partial OSC/CSI sequence, but keep accumulated semantic state
        // (cwd/command phase/executable) - a child hard-reset (RIS) does not imply the shell's
        // last-known working directory or command lifecycle became invalid.
        self.semantic_parser = SemanticVteParser::new();
        self.mouse_mode = MouseModeState::default();
        self.pixel_mouse = false;
        self.scrollback_offset = 0;
        self.cache = TerminalRenderSnapshot::default();
        self.semantic_marks.clear();
        self.semantic_events_seen = 0;
        self.prompt_boundary_scanner = PromptBoundaryScanner::default();
        self.active_prompt_mark = None;
        self.evicted_lines = 0;
        self.history_epoch = self.history_epoch.saturating_add(1);
        self.alt_screen = false;
        // Images, unlike semantic state, are screen contents: a hard reset clears them with the
        // grid they were drawn on.
        #[cfg(feature = "terminal-images")]
        {
            self.graphics_scanner.reset();
            self.graphics.reset();
            self.graphics_alt_screen = false;
        }
        self.dirty = true;
    }

    /// Get current mouse mode state.
    pub fn mouse_mode(&self) -> MouseModeState {
        self.mouse_mode
    }

    /// Get the input-affecting DEC private modes the child has enabled.
    ///
    /// Pass this to [`key_event_to_bytes`](super::key_event_to_bytes) and
    /// [`encode_paste`](super::encode_paste) when wiring a `TerminalPty` by hand.
    pub fn key_modes(&self) -> TerminalKeyModes {
        key_modes_from_term(*self.term.mode())
    }

    /// The window title the program has set via OSC 0/2 (e.g. the shell's
    /// `$PWD` or a running program's name). Returns `None` if no title has been
    /// set or it was reset. Updated as bytes are processed.
    pub fn title(&self) -> Option<String> {
        self.listener.title.borrow().clone()
    }

    fn export_primary_replay(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x1bc");
        bytes.extend_from_slice(&self.export_active_grid_repaint(true));
        self.push_cursor_position(&mut bytes);
        self.push_title(&mut bytes);
        self.push_modes(&mut bytes);
        bytes
    }

    fn export_active_grid_repaint(&self, include_scrollback: bool) -> Vec<u8> {
        let grid = self.term.grid();
        let top = if include_scrollback {
            grid.topmost_line()
        } else {
            Line(0)
        };
        let bottom = grid.bottommost_line();
        let mut bytes = Vec::new();
        // No ED 2 here: on alacritty's primary screen it scrolls the cleared
        // viewport into history, adding a phantom scrollback row. The preceding
        // RIS (primary) or DECSET 1049 (alt) already blanks the target grid.
        bytes.extend_from_slice(b"\x1b[0m\x1b[H");
        let mut style = ReplayStyle::default();

        for line in top.0..=bottom.0 {
            let line = Line(line);
            let wrapline = grid[line][grid.last_column()]
                .flags
                .contains(CellFlags::WRAPLINE);
            let end_col = if wrapline {
                grid.columns()
            } else {
                (0..grid.columns())
                    .rfind(|col| !grid[line][Column(*col)].is_empty())
                    .map_or(0, |col| col + 1)
            };
            for col in 0..end_col {
                let cell = &grid[line][Column(col)];
                if cell
                    .flags
                    .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let next_style = ReplayStyle::from(cell);
                if next_style != style {
                    next_style.push_sgr(&mut bytes);
                    style = next_style;
                }
                push_cell_text(&mut bytes, cell);
            }
            if line != bottom && !wrapline {
                if style != ReplayStyle::default() {
                    ReplayStyle::default().push_sgr(&mut bytes);
                    style = ReplayStyle::default();
                }
                bytes.extend_from_slice(b"\r\n");
            }
        }
        bytes.extend_from_slice(b"\x1b[0m");
        bytes
    }

    fn push_cursor_position(&self, bytes: &mut Vec<u8>) {
        let cursor = &self.term.grid().cursor;
        let row = (cursor.point.line.0.max(0) as usize + 1).min(self.rows as usize);
        let col = (cursor.point.column.0 + 1).min(self.cols as usize);
        bytes.extend_from_slice(format!("\x1b[{row};{col}H").as_bytes());
        ReplayStyle::from(&cursor.template).push_sgr(bytes);
    }

    fn push_title(&self, bytes: &mut Vec<u8>) {
        if let Some(title) = self.title().filter(|title| !title.is_empty()) {
            bytes.extend_from_slice(b"\x1b]2;");
            bytes.extend_from_slice(title.as_bytes());
            bytes.extend_from_slice(b"\x1b\\");
        }
    }

    fn push_modes(&self, bytes: &mut Vec<u8>) {
        let mode = *self.term.mode();
        push_dec_mode(bytes, 1, mode.contains(TermMode::APP_CURSOR));
        push_dec_mode(bytes, 7, mode.contains(TermMode::LINE_WRAP));
        push_dec_mode(bytes, 25, mode.contains(TermMode::SHOW_CURSOR));
        push_dec_mode(bytes, 1000, mode.contains(TermMode::MOUSE_REPORT_CLICK));
        push_dec_mode(bytes, 1002, mode.contains(TermMode::MOUSE_DRAG));
        push_dec_mode(bytes, 1003, mode.contains(TermMode::MOUSE_MOTION));
        push_dec_mode(bytes, 1004, mode.contains(TermMode::FOCUS_IN_OUT));
        push_dec_mode(bytes, 1005, mode.contains(TermMode::UTF8_MOUSE));
        push_dec_mode(bytes, 1006, mode.contains(TermMode::SGR_MOUSE));
        push_dec_mode(bytes, SGR_PIXELS_MOUSE, self.pixel_mouse);
        push_dec_mode(bytes, 2004, mode.contains(TermMode::BRACKETED_PASTE));
        let kitty_flags = u8::from(mode.contains(TermMode::DISAMBIGUATE_ESC_CODES))
            | (u8::from(mode.contains(TermMode::REPORT_EVENT_TYPES)) << 1)
            | (u8::from(mode.contains(TermMode::REPORT_ALTERNATE_KEYS)) << 2)
            | (u8::from(mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC)) << 3)
            | (u8::from(mode.contains(TermMode::REPORT_ASSOCIATED_TEXT)) << 4);
        if kitty_flags != 0 {
            bytes.extend_from_slice(format!("\x1b[>{kitty_flags}u").as_bytes());
        }
        bytes.extend_from_slice(if mode.contains(TermMode::APP_KEYPAD) {
            b"\x1b="
        } else {
            b"\x1b>"
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReplayStyle {
    fg: TermColor,
    bg: TermColor,
    flags: CellFlags,
    underline_color: Option<TermColor>,
}

impl Default for ReplayStyle {
    fn default() -> Self {
        Self {
            fg: TermColor::Named(NamedColor::Foreground),
            bg: TermColor::Named(NamedColor::Background),
            flags: CellFlags::empty(),
            underline_color: None,
        }
    }
}

impl From<&TermCell> for ReplayStyle {
    fn from(cell: &TermCell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            flags: cell.flags
                & (CellFlags::BOLD
                    | CellFlags::DIM
                    | CellFlags::ITALIC
                    | CellFlags::ALL_UNDERLINES
                    | CellFlags::INVERSE
                    | CellFlags::HIDDEN
                    | CellFlags::STRIKEOUT),
            underline_color: cell.underline_color(),
        }
    }
}

impl ReplayStyle {
    fn push_sgr(self, bytes: &mut Vec<u8>) {
        let mut params = vec!["0".to_string()];
        let flags = self.flags;
        if flags.contains(CellFlags::BOLD) {
            params.push("1".to_string());
        }
        if flags.contains(CellFlags::DIM) {
            params.push("2".to_string());
        }
        if flags.contains(CellFlags::ITALIC) {
            params.push("3".to_string());
        }
        if flags.contains(CellFlags::DOUBLE_UNDERLINE) {
            params.push("4:2".to_string());
        } else if flags.contains(CellFlags::UNDERCURL) {
            params.push("4:3".to_string());
        } else if flags.contains(CellFlags::DOTTED_UNDERLINE) {
            params.push("4:4".to_string());
        } else if flags.contains(CellFlags::DASHED_UNDERLINE) {
            params.push("4:5".to_string());
        } else if flags.contains(CellFlags::UNDERLINE) {
            params.push("4".to_string());
        }
        if flags.contains(CellFlags::INVERSE) {
            params.push("7".to_string());
        }
        if flags.contains(CellFlags::HIDDEN) {
            params.push("8".to_string());
        }
        if flags.contains(CellFlags::STRIKEOUT) {
            params.push("9".to_string());
        }
        push_color_sgr(&mut params, self.fg, true);
        push_color_sgr(&mut params, self.bg, false);
        if let Some(color) = self.underline_color {
            push_underline_color_sgr(&mut params, color);
        }
        bytes.extend_from_slice(format!("\x1b[{}m", params.join(";")).as_bytes());
    }
}

fn push_dec_mode(bytes: &mut Vec<u8>, mode: u16, enabled: bool) {
    let suffix = if enabled { 'h' } else { 'l' };
    bytes.extend_from_slice(format!("\x1b[?{mode}{suffix}").as_bytes());
}

fn push_cell_text(bytes: &mut Vec<u8>, cell: &TermCell) {
    let mut buf = [0; 4];
    bytes.extend_from_slice(cell.c.encode_utf8(&mut buf).as_bytes());
    if let Some(zerowidth) = cell.zerowidth() {
        for ch in zerowidth {
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }
}

fn push_color_sgr(params: &mut Vec<String>, color: TermColor, foreground: bool) {
    match color {
        TermColor::Named(named) => {
            let value = named_color_sgr(named, foreground);
            params.push(value.to_string());
        }
        TermColor::Indexed(index) => {
            params.push(if foreground { "38" } else { "48" }.to_string());
            params.push("5".to_string());
            params.push(index.to_string());
        }
        TermColor::Spec(TermRgb { r, g, b }) => {
            params.push(if foreground { "38" } else { "48" }.to_string());
            params.push("2".to_string());
            params.push(r.to_string());
            params.push(g.to_string());
            params.push(b.to_string());
        }
    }
}

fn push_underline_color_sgr(params: &mut Vec<String>, color: TermColor) {
    match color {
        TermColor::Named(named) => {
            if let Some(index) = named_color_index(named) {
                params.push("58".to_string());
                params.push("5".to_string());
                params.push(index.to_string());
            }
        }
        TermColor::Indexed(index) => {
            params.push("58".to_string());
            params.push("5".to_string());
            params.push(index.to_string());
        }
        TermColor::Spec(TermRgb { r, g, b }) => {
            params.push("58".to_string());
            params.push("2".to_string());
            params.push(r.to_string());
            params.push(g.to_string());
            params.push(b.to_string());
        }
    }
}

fn named_color_sgr(color: NamedColor, foreground: bool) -> u16 {
    match color {
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => 39,
        NamedColor::Background => 49,
        NamedColor::Black | NamedColor::DimBlack => {
            if foreground {
                30
            } else {
                40
            }
        }
        NamedColor::Red | NamedColor::DimRed => {
            if foreground {
                31
            } else {
                41
            }
        }
        NamedColor::Green | NamedColor::DimGreen => {
            if foreground {
                32
            } else {
                42
            }
        }
        NamedColor::Yellow | NamedColor::DimYellow => {
            if foreground {
                33
            } else {
                43
            }
        }
        NamedColor::Blue | NamedColor::DimBlue => {
            if foreground {
                34
            } else {
                44
            }
        }
        NamedColor::Magenta | NamedColor::DimMagenta => {
            if foreground {
                35
            } else {
                45
            }
        }
        NamedColor::Cyan | NamedColor::DimCyan => {
            if foreground {
                36
            } else {
                46
            }
        }
        NamedColor::White | NamedColor::DimWhite => {
            if foreground {
                37
            } else {
                47
            }
        }
        NamedColor::BrightBlack => {
            if foreground {
                90
            } else {
                100
            }
        }
        NamedColor::BrightRed => {
            if foreground {
                91
            } else {
                101
            }
        }
        NamedColor::BrightGreen => {
            if foreground {
                92
            } else {
                102
            }
        }
        NamedColor::BrightYellow => {
            if foreground {
                93
            } else {
                103
            }
        }
        NamedColor::BrightBlue => {
            if foreground {
                94
            } else {
                104
            }
        }
        NamedColor::BrightMagenta => {
            if foreground {
                95
            } else {
                105
            }
        }
        NamedColor::BrightCyan => {
            if foreground {
                96
            } else {
                106
            }
        }
        NamedColor::BrightWhite => {
            if foreground {
                97
            } else {
                107
            }
        }
        NamedColor::Cursor => {
            if foreground {
                39
            } else {
                49
            }
        }
    }
}

fn named_color_index(color: NamedColor) -> Option<u8> {
    match color {
        NamedColor::Black | NamedColor::DimBlack => Some(0),
        NamedColor::Red | NamedColor::DimRed => Some(1),
        NamedColor::Green | NamedColor::DimGreen => Some(2),
        NamedColor::Yellow | NamedColor::DimYellow => Some(3),
        NamedColor::Blue | NamedColor::DimBlue => Some(4),
        NamedColor::Magenta | NamedColor::DimMagenta => Some(5),
        NamedColor::Cyan | NamedColor::DimCyan => Some(6),
        NamedColor::White | NamedColor::DimWhite => Some(7),
        NamedColor::BrightBlack => Some(8),
        NamedColor::BrightRed => Some(9),
        NamedColor::BrightGreen => Some(10),
        NamedColor::BrightYellow => Some(11),
        NamedColor::BrightBlue => Some(12),
        NamedColor::BrightMagenta => Some(13),
        NamedColor::BrightCyan => Some(14),
        NamedColor::BrightWhite => Some(15),
        NamedColor::Foreground
        | NamedColor::Background
        | NamedColor::Cursor
        | NamedColor::BrightForeground
        | NamedColor::DimForeground => None,
    }
}

fn renderable_content_lines(
    display_iter: alacritty_terminal::grid::GridIterator<'_, TermCell>,
    display_offset: usize,
    rows: u16,
    cols: u16,
    palette: TerminalColorPalette,
) -> Vec<Vec<Span>> {
    let mut lines: Vec<Vec<Span>> = vec![Vec::new(); rows as usize];
    let mut current_row: Option<usize> = None;
    let mut run_style: Option<Style> = None;
    let mut run_text = String::new();

    let flush_run = |row: usize,
                     run_style: &mut Option<Style>,
                     run_text: &mut String,
                     lines: &mut Vec<Vec<Span>>| {
        if run_text.is_empty() {
            *run_style = None;
            return;
        }
        if let Some(style) = run_style.take() {
            lines[row].push(Span::new(std::mem::take(run_text)).style(style));
        } else {
            lines[row].push(Span::new(std::mem::take(run_text)));
        }
    };

    for indexed in display_iter {
        let Some(point) = term::point_to_viewport(display_offset, indexed.point) else {
            continue;
        };
        if point.line >= rows as usize || point.column.0 >= cols as usize {
            continue;
        }

        let row = point.line;
        if current_row != Some(row) {
            if let Some(prev_row) = current_row {
                flush_run(prev_row, &mut run_style, &mut run_text, &mut lines);
            }
            current_row = Some(row);
        }

        let cell = indexed.cell;
        if cell
            .flags
            .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }

        let style = style_from_term_cell(cell, &palette);
        if run_style != Some(style) {
            if let Some(prev_row) = current_row {
                flush_run(prev_row, &mut run_style, &mut run_text, &mut lines);
            }
            run_style = Some(style);
        }
        push_cell_text_str(&mut run_text, cell);
    }

    if let Some(row) = current_row {
        flush_run(row, &mut run_style, &mut run_text, &mut lines);
    }

    for line in &mut lines {
        if line.is_empty() {
            line.push(Span::new(""));
        }
    }

    lines
}

fn push_cell_text_str(out: &mut String, cell: &TermCell) {
    // An image placeholder is not text: it names a picture the renderer paints over these cells.
    // Passing it through would put a tofu box under every image, and would put the character into
    // anything that reads the snapshot - a search, a copy, an exported log.
    #[cfg(feature = "terminal-images")]
    if cell.c == PLACEHOLDER {
        out.push(' ');
        return;
    }
    let ch = if cell.flags.contains(CellFlags::HIDDEN) {
        ' '
    } else {
        cell.c
    };
    out.push(ch);
    if let Some(zerowidth) = cell.zerowidth() {
        for ch in zerowidth {
            out.push(*ch);
        }
    }
}

/// Soft-wrap flags for the `rows` visible viewport rows at `display_offset`, one per row.
///
/// The terminal records a wrap on the last cell of the row that overflowed, so the flag reads
/// forward: row `i` is `true` when its text continues on row `i + 1`.
fn visible_wrapped_rows(
    grid: &alacritty_terminal::grid::Grid<TermCell>,
    display_offset: usize,
    rows: u16,
) -> Vec<bool> {
    let last_column = grid.last_column();
    (0..usize::from(rows))
        .map(|row| {
            let Ok(offset) = i32::try_from(display_offset) else {
                return false;
            };
            let Ok(row) = i32::try_from(row) else {
                return false;
            };
            let line = Line(row - offset);
            line >= grid.topmost_line()
                && line <= grid.bottommost_line()
                && grid[line][last_column].flags.contains(CellFlags::WRAPLINE)
        })
        .collect()
}

fn display_line_width(grid: &alacritty_terminal::grid::Grid<TermCell>, line: Line) -> usize {
    let wrapline = grid[line][grid.last_column()]
        .flags
        .contains(CellFlags::WRAPLINE);
    if wrapline {
        return grid.columns();
    }
    (0..grid.columns())
        .rfind(|col| {
            let cell = &grid[line][Column(*col)];
            !cell
                .flags
                .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
                && !cell.is_empty()
        })
        .map_or(0, |col| col + 1)
}

fn display_columns_text(
    grid: &alacritty_terminal::grid::Grid<TermCell>,
    line: Line,
    start_col: usize,
    end_col: usize,
) -> String {
    let width = display_line_width(grid, line);
    let col_start = start_col.min(width);
    let col_end = end_col.min(width);
    if col_start >= col_end {
        return String::new();
    }

    let mut result = String::new();
    let mut display_col = 0usize;
    for col in 0..grid.columns() {
        let cell = &grid[line][Column(col)];
        if cell
            .flags
            .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let cell_width = if cell.flags.contains(CellFlags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let cell_end = display_col.saturating_add(cell_width);
        if cell_end > col_start && display_col < col_end {
            push_cell_text_str(&mut result, cell);
        }
        display_col = cell_end;
        if display_col >= col_end {
            break;
        }
    }
    result
}

/// The low 24 bits of the image id a placeholder cell names, from its foreground colour.
///
/// The protocol puts the id in the colour so a row of placeholders needs one escape sequence
/// rather than one per cell. A cell with no explicit foreground names no image.
#[cfg(feature = "terminal-images")]
fn placeholder_id(cell: &TermCell) -> Option<u32> {
    match cell.fg {
        TermColor::Spec(rgb) => {
            Some((u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b))
        }
        TermColor::Indexed(index) => Some(u32::from(index)),
        TermColor::Named(_) => None,
    }
}

fn push_plain_line_text(
    grid: &alacritty_terminal::grid::Grid<TermCell>,
    line: Line,
    out: &mut String,
) {
    let wrapline = grid[line][grid.last_column()]
        .flags
        .contains(CellFlags::WRAPLINE);
    let end_col = if wrapline {
        grid.columns()
    } else {
        (0..grid.columns())
            .rfind(|col| !grid[line][Column(*col)].is_empty())
            .map_or(0, |col| col + 1)
    };
    for col in 0..end_col {
        let cell = &grid[line][Column(col)];
        if cell
            .flags
            .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        push_cell_text_str(out, cell);
    }
}

fn key_modes_from_term(mode: TermMode) -> TerminalKeyModes {
    TerminalKeyModes {
        app_cursor: mode.contains(TermMode::APP_CURSOR),
        bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        kitty_keyboard: KittyKeyboardFlags {
            disambiguate_escape_codes: mode.contains(TermMode::DISAMBIGUATE_ESC_CODES),
            report_event_types: mode.contains(TermMode::REPORT_EVENT_TYPES),
            report_alternate_keys: mode.contains(TermMode::REPORT_ALTERNATE_KEYS),
            report_all_keys_as_escape_codes: mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC),
            report_associated_text: mode.contains(TermMode::REPORT_ASSOCIATED_TEXT),
        },
    }
}

fn mouse_mode_from_term(mode: TermMode, pixel_mouse: bool) -> MouseModeState {
    // 1016 carries SGR's shape, so it supersedes the encoding rather than combining with it, and a
    // program that sets it without 1006 still gets SGR-shaped reports.
    let encoding = if pixel_mouse {
        MouseEncoding::SgrPixels
    } else if mode.contains(TermMode::SGR_MOUSE) {
        MouseEncoding::Sgr
    } else if mode.contains(TermMode::UTF8_MOUSE) {
        MouseEncoding::Utf8
    } else {
        MouseEncoding::X10
    };

    let mouse_mode = if mode.contains(TermMode::MOUSE_MOTION) {
        MouseMode::AnyEvent
    } else if mode.contains(TermMode::MOUSE_DRAG) || mode.contains(TermMode::MOUSE_REPORT_CLICK) {
        MouseMode::Normal
    } else {
        MouseMode::None
    };

    let focus_events_enabled = mode.contains(TermMode::FOCUS_IN_OUT);

    MouseModeState {
        mode: mouse_mode,
        encoding,
        focus_events_enabled,
    }
}

fn style_from_term_cell(cell: &TermCell, palette: &TerminalColorPalette) -> Style {
    let fg = map_term_color(cell.fg, palette).map(Into::into);
    let bg = map_term_color(cell.bg, palette).map(Into::into);
    let flags = cell.flags;

    Style {
        fg,
        bg,
        fg_transform: None,
        bg_transform: None,
        contrast_policy: None,
        bold: Some(flags.contains(CellFlags::BOLD)),
        dim: Some(flags.contains(CellFlags::DIM)),
        italic: Some(flags.contains(CellFlags::ITALIC)),
        underline: Some(flags.intersects(CellFlags::ALL_UNDERLINES)),
        reverse: Some(flags.contains(CellFlags::INVERSE)),
        dim_amount: None,
        strikethrough: Some(flags.contains(CellFlags::STRIKEOUT)),
        underline_color: None,
        tint: None,
    }
}

fn map_term_color(color: TermColor, palette: &TerminalColorPalette) -> Option<UiColor> {
    match color {
        TermColor::Named(named) => map_named_color(named, palette),
        TermColor::Spec(TermRgb { r, g, b }) => Some(UiColor::Rgb(r, g, b)),
        TermColor::Indexed(index) if usize::from(index) < palette.ansi.len() => {
            Some(palette.ansi[usize::from(index)])
        }
        TermColor::Indexed(index) => Some(UiColor::Indexed(index)),
    }
}

fn map_named_color(color: NamedColor, palette: &TerminalColorPalette) -> Option<UiColor> {
    match color {
        NamedColor::Black => Some(palette.ansi[0]),
        NamedColor::Red => Some(palette.ansi[1]),
        NamedColor::Green => Some(palette.ansi[2]),
        NamedColor::Yellow => Some(palette.ansi[3]),
        NamedColor::Blue => Some(palette.ansi[4]),
        NamedColor::Magenta => Some(palette.ansi[5]),
        NamedColor::Cyan => Some(palette.ansi[6]),
        NamedColor::White => Some(palette.ansi[7]),
        NamedColor::BrightBlack => Some(palette.ansi[8]),
        NamedColor::BrightRed => Some(palette.ansi[9]),
        NamedColor::BrightGreen => Some(palette.ansi[10]),
        NamedColor::BrightYellow => Some(palette.ansi[11]),
        NamedColor::BrightBlue => Some(palette.ansi[12]),
        NamedColor::BrightMagenta => Some(palette.ansi[13]),
        NamedColor::BrightCyan => Some(palette.ansi[14]),
        NamedColor::BrightWhite => Some(palette.ansi[15]),
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => {
            palette.foreground
        }
        NamedColor::Background => palette.background,
        NamedColor::Cursor
        | NamedColor::DimBlack
        | NamedColor::DimRed
        | NamedColor::DimGreen
        | NamedColor::DimYellow
        | NamedColor::DimBlue
        | NamedColor::DimMagenta
        | NamedColor::DimCyan
        | NamedColor::DimWhite => None,
    }
}

fn default_ansi_palette() -> [UiColor; 16] {
    [
        UiColor::Black,
        UiColor::Red,
        UiColor::Green,
        UiColor::Yellow,
        UiColor::Blue,
        UiColor::Magenta,
        UiColor::Cyan,
        UiColor::Gray,
        UiColor::DarkGray,
        UiColor::LightRed,
        UiColor::LightGreen,
        UiColor::LightYellow,
        UiColor::LightBlue,
        UiColor::LightMagenta,
        UiColor::LightCyan,
        UiColor::White,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_replay_round_trips(source: &mut TerminalScreen) -> TerminalScreen {
        let replay = source.export_replay_bytes();
        let mut target = TerminalScreen::new(source.rows, source.cols, source.scrollback_len);
        target.set_palette(source.palette());
        target.process_bytes(&replay);
        assert!(target.drain_responses().is_empty());

        let source_snapshot = source.render_snapshot();
        let target_snapshot = target.render_snapshot();
        assert_eq!(target_snapshot.text, source_snapshot.text);
        assert_eq!(target_snapshot.color_lines, source_snapshot.color_lines);
        assert_eq!(target_snapshot.cursor_row, source_snapshot.cursor_row);
        assert_eq!(target_snapshot.cursor_col, source_snapshot.cursor_col);
        assert_eq!(
            target_snapshot.cursor_visible,
            source_snapshot.cursor_visible
        );
        assert_eq!(target_snapshot.mouse_mode, source_snapshot.mouse_mode);
        assert_eq!(target_snapshot.key_modes, source_snapshot.key_modes);
        assert_eq!(target.title(), source.title());
        target
    }

    fn assert_scrollback_views_round_trip(
        source: &mut TerminalScreen,
        target: &mut TerminalScreen,
    ) {
        let total_scrollback_rows = source.total_scrollback_rows();
        assert_eq!(target.total_scrollback_rows(), total_scrollback_rows);

        for offset in 0..=total_scrollback_rows {
            source.set_scrollback(offset);
            target.set_scrollback(offset);
            let source_snapshot = source.render_snapshot();
            let target_snapshot = target.render_snapshot();
            assert_eq!(
                target_snapshot.text, source_snapshot.text,
                "offset {offset}"
            );
            assert_eq!(
                target_snapshot.color_lines, source_snapshot.color_lines,
                "offset {offset}"
            );
        }

        source.set_scrollback(0);
        target.set_scrollback(0);
    }

    fn span_fg(snapshot: &TerminalRenderSnapshot, span_index: usize) -> Option<UiColor> {
        snapshot.color_lines[0][span_index]
            .style
            .fg
            .map(|paint| paint.color())
    }

    fn span_bg(snapshot: &TerminalRenderSnapshot, span_index: usize) -> Option<UiColor> {
        snapshot.color_lines[0][span_index]
            .style
            .bg
            .map(|paint| paint.color())
    }

    #[test]
    fn snapshot_selection_text_uses_display_columns_and_inclusive_endpoints() {
        let snapshot = TerminalRenderSnapshot::from_parts(
            "a界🙂b",
            vec![vec![Span::new("a界🙂b")]],
            0,
            0,
            true,
            CaretShape::Block,
            true,
            7,
            0,
            0,
            MouseModeState::default(),
            TerminalKeyModes::default(),
        );

        assert_eq!(
            snapshot.selection_text(
                &GridSelection {
                    anchor: GridPos { row: 0, col: 1 },
                    cursor: GridPos { row: 0, col: 2 },
                },
                SelectionEnd::Inclusive,
                true,
            ),
            "界"
        );
        assert_eq!(
            snapshot.selection_text(
                &GridSelection {
                    anchor: GridPos { row: 0, col: 3 },
                    cursor: GridPos { row: 0, col: 4 },
                },
                SelectionEnd::Inclusive,
                true,
            ),
            "🙂"
        );

        let trailing = TerminalRenderSnapshot::from_parts(
            "a  ",
            vec![vec![Span::new("a  ")]],
            0,
            0,
            true,
            CaretShape::Block,
            true,
            7,
            0,
            0,
            MouseModeState::default(),
            TerminalKeyModes::default(),
        );
        let trailing_selection = GridSelection {
            anchor: GridPos { row: 0, col: 0 },
            cursor: GridPos { row: 0, col: 2 },
        };
        assert_eq!(
            trailing.selection_text(&trailing_selection, SelectionEnd::Inclusive, false),
            "a  "
        );
        assert_eq!(
            trailing.selection_text(&trailing_selection, SelectionEnd::Inclusive, true),
            "a"
        );
    }

    #[test]
    fn decorated_snapshot_keeps_plain_text_and_hashes_decorations() {
        let snapshot = TerminalRenderSnapshot::from_parts(
            "abc",
            vec![vec![Span::new("abc")]],
            0,
            0,
            true,
            CaretShape::Block,
            true,
            7,
            0,
            0,
            MouseModeState::default(),
            TerminalKeyModes::default(),
        );
        let decoration = TerminalDecoration::highlight(0, 1..2, Style::new().bold());
        let decorated = snapshot.decorated(std::slice::from_ref(&decoration));
        let repeated = snapshot.decorated(std::slice::from_ref(&decoration));

        assert_eq!(decorated.text, snapshot.text);
        assert_ne!(decorated.sequence, snapshot.sequence);
        assert_eq!(decorated.sequence, repeated.sequence);
        assert_eq!(decorated.color_lines[0][0].content.as_ref(), "a");
        assert_eq!(decorated.color_lines[0][1].content.as_ref(), "b");
        assert_eq!(decorated.color_lines[0][1].style.bold, Some(true));
    }

    #[test]
    fn decorated_snapshot_uses_sorted_overlap_precedence_and_right_to_left_labels() {
        let snapshot = TerminalRenderSnapshot::from_parts(
            "abcd",
            vec![vec![Span::new("abcd")]],
            0,
            0,
            true,
            CaretShape::Block,
            true,
            7,
            0,
            0,
            MouseModeState::default(),
            TerminalKeyModes::default(),
        );
        let red = TerminalDecoration::highlight(0, 0..3, Style::new().fg(UiColor::Red));
        let blue = TerminalDecoration::highlight(0, 1..2, Style::new().fg(UiColor::Blue));
        let decorated = snapshot.decorated(&[red, blue]);
        assert_eq!(
            decorated.color_lines[0][0].style.fg,
            Some(UiColor::Red.into())
        );
        assert_eq!(
            decorated.color_lines[0][1].style.fg,
            Some(UiColor::Blue.into())
        );
        assert_eq!(
            decorated.color_lines[0][2].style.fg,
            Some(UiColor::Red.into())
        );

        let labels = snapshot.decorated(&[
            TerminalDecoration::label(0, 1, Span::new("X")),
            TerminalDecoration::label(0, 3, Span::new("Y")),
        ]);
        let text: String = labels.color_lines[0]
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "aXbcYd");
    }

    #[test]
    fn overlay_decorations_paint_over_columns_without_moving_the_row() {
        let snapshot = TerminalRenderSnapshot::from_parts(
            "abcd",
            vec![vec![Span::new("abcd")]],
            0,
            0,
            true,
            CaretShape::Block,
            true,
            7,
            0,
            0,
            MouseModeState::default(),
            TerminalKeyModes::default(),
        );
        // An insert at the last column would land past a full-width row's right edge; an overlay
        // marks the same column and leaves the row exactly as wide as it was.
        let decorated = snapshot.decorated(&[
            TerminalDecoration::overlay(0, 0, Span::new("XY")),
            TerminalDecoration::overlay(0, 3, Span::new("Z")),
        ]);
        let text: String = decorated.color_lines[0]
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "XYcZ");
    }

    #[test]
    fn render_snapshot_reports_soft_wrapped_rows() {
        let mut screen = TerminalScreen::new(4, 6, 10);
        screen.process_bytes(b"abcdefgh\r\nshort");
        let snapshot = screen.render_snapshot();

        // Row 0 overflowed into row 1; row 2 is a line the program broke itself.
        assert_eq!(snapshot.text.as_ref(), "abcdef\ngh    \nshort \n      ");
        assert_eq!(
            snapshot.wrapped_rows.as_ref(),
            &[true, false, false, false][..]
        );
    }

    #[test]
    fn narrowing_a_wrapped_live_line_does_not_duplicate_its_prefix() {
        let line = concat!(
            "tui-lipan main ❯ ops::profile::tests::capturing_an_ephemeral_session_",
            "does_not_rename_it fails in the full-suite run and passes in isolation. ",
            "It is not mine — I stashed my changes and reproduced the identical failure ",
            "on the baseline tree. Its rx.try_recv().is_err() assertion is racy under parallel load."
        );
        let mut screen = TerminalScreen::new(4, 176, 20);
        screen.process_bytes(b"\x1b]13");
        screen.process_bytes(format!("3;A\x1b\\{line}").as_bytes());

        screen.resize(4, 147);
        assert!(
            screen
                .render_snapshot()
                .text
                .lines()
                .all(|row| row.trim().is_empty()),
            "the stale active prompt must be removed before reflow"
        );

        // Readline redraws the active input after receiving SIGWINCH.
        screen.process_bytes(b"\r\x1b[K\r\x1b[A\x1b[K\r");
        screen.process_bytes(line.as_bytes());
        let snapshot = screen.render_snapshot();
        let actual = snapshot
            .text
            .lines()
            .map(|row| row.trim_end().to_owned())
            .collect::<Vec<_>>();
        let mut expected = line
            .chars()
            .collect::<Vec<_>>()
            .chunks(147)
            .map(|chunk| chunk.iter().collect::<String>().trim_end().to_owned())
            .collect::<Vec<_>>();
        expected.resize(4, String::new());

        assert_eq!(actual, expected);
    }

    #[test]
    fn resizing_after_command_start_preserves_output() {
        let mut screen = TerminalScreen::new(4, 24, 20);
        screen.process_bytes(
            b"\x1b]133;A\x1b\\prompt \x1b]133;B\x1b\\command\r\n\
              \x1b]133;C\x1b\\command output",
        );

        screen.resize(4, 18);

        assert!(
            screen.render_snapshot().text.contains("command output"),
            "OSC 133;C must retire the prompt mark before command output starts"
        );
    }

    #[test]
    fn repeated_decorations_have_distinct_sequence_from_single_decoration() {
        let snapshot = TerminalRenderSnapshot::from_parts(
            "abc",
            vec![vec![Span::new("abc")]],
            0,
            0,
            true,
            CaretShape::Block,
            true,
            7,
            0,
            0,
            MouseModeState::default(),
            TerminalKeyModes::default(),
        );
        let decoration = TerminalDecoration::highlight(0, 1..2, Style::new().bold());
        let once = snapshot.decorated(std::slice::from_ref(&decoration));
        let twice = snapshot.decorated(&[decoration.clone(), decoration]);
        let mut different_source = snapshot.clone();
        different_source.sequence = 8;
        let same_decoration = TerminalDecoration::highlight(0, 1..2, Style::new().bold());
        let different = different_source.decorated(std::slice::from_ref(&same_decoration));

        assert_ne!(once.sequence, twice.sequence);
        assert_ne!(twice.sequence, snapshot.sequence);
        assert_ne!(once.sequence, different.sequence);
    }

    #[test]
    fn export_selection_text_is_char_indexed_and_uses_trimmed_lines() {
        let mut screen = TerminalScreen::new(1, 20, 10);
        screen.process_bytes("a界🙂b   ".as_bytes());

        assert_eq!(
            screen.export_selection_text(
                GridPos { row: 0, col: 1 },
                GridPos { row: 0, col: 2 },
                SelectionEnd::Inclusive,
            ),
            "界🙂"
        );
        assert_eq!(
            screen.export_selection_text(
                GridPos { row: 0, col: 2 },
                GridPos { row: 0, col: 3 },
                SelectionEnd::Inclusive,
            ),
            "🙂b"
        );
    }

    #[test]
    fn bell_count_starts_at_zero() {
        let screen = TerminalScreen::new(2, 8, 10);

        assert_eq!(screen.bell_count(), 0);
    }

    #[test]
    fn bell_count_tracks_each_bel() {
        let mut screen = TerminalScreen::new(2, 8, 10);

        screen.process_bytes(b"\x07text\x07\x07");

        assert_eq!(screen.bell_count(), 3);
    }

    #[test]
    fn bell_count_ignores_non_bel_input() {
        let mut screen = TerminalScreen::new(2, 8, 10);

        screen.process_bytes(b"text\r\n\x1b[31mred\x1b[0m");

        assert_eq!(screen.bell_count(), 0);
    }

    #[test]
    fn osc52_stores_are_decoded_and_drained() {
        let mut screen = TerminalScreen::new(2, 8, 10);

        screen.process_bytes(b"\x1b]52;c;aGVsbG8=\x07\x1b]52;p;d29ybGQ=\x1b\\");

        assert_eq!(
            screen.drain_clipboard_events(),
            vec![
                TerminalClipboardEvent {
                    target: TerminalClipboardTarget::Clipboard,
                    text: "hello".to_string(),
                },
                TerminalClipboardEvent {
                    target: TerminalClipboardTarget::Selection,
                    text: "world".to_string(),
                },
            ]
        );
        assert!(screen.drain_clipboard_events().is_empty());
    }

    #[test]
    fn osc52_loads_remain_disabled() {
        let mut screen = TerminalScreen::new(2, 8, 10);

        screen.process_bytes(b"\x1b]52;c;?\x07");

        assert!(screen.drain_clipboard_events().is_empty());
        assert!(screen.drain_responses().is_empty());
    }

    #[test]
    fn palette_resolves_named_and_indexed_ansi_slots() {
        let mut screen = TerminalScreen::new(2, 8, 10);
        let mut ansi = default_ansi_palette();
        ansi[1] = UiColor::Rgb(1, 2, 3);
        ansi[2] = UiColor::Rgb(4, 5, 6);
        screen.set_palette(TerminalColorPalette::default().ansi(ansi));

        screen.process_bytes(b"\x1b[31mR\x1b[38;5;2mG");
        let snapshot = screen.render_snapshot();

        assert_eq!(span_fg(&snapshot, 0), Some(UiColor::Rgb(1, 2, 3)));
        assert_eq!(span_fg(&snapshot, 1), Some(UiColor::Rgb(4, 5, 6)));
    }

    #[test]
    fn palette_resolves_default_foreground_and_background() {
        let mut screen = TerminalScreen::new(2, 8, 10);
        screen.set_palette(TerminalColorPalette::new(
            UiColor::Rgb(10, 20, 30),
            UiColor::Rgb(40, 50, 60),
            default_ansi_palette(),
        ));

        screen.process_bytes(b"X");
        let snapshot = screen.render_snapshot();

        assert_eq!(span_fg(&snapshot, 0), Some(UiColor::Rgb(10, 20, 30)));
        assert_eq!(span_bg(&snapshot, 0), Some(UiColor::Rgb(40, 50, 60)));
    }

    #[test]
    fn palette_from_host_colors_preserves_host_foreground_and_ansi_slots() {
        let ansi = std::array::from_fn(|i| UiColor::Rgb(i as u8, 10 + i as u8, 20 + i as u8));
        let colors = HostTerminalColors {
            ansi,
            fg: UiColor::Rgb(230, 231, 232),
            bg: UiColor::Rgb(10, 11, 12),
        };
        let pane_background = UiColor::Rgb(1, 2, 3);

        let palette = TerminalColorPalette::from_host_colors(colors, pane_background);

        assert_eq!(palette.foreground, Some(colors.fg));
        assert_eq!(palette.background, Some(pane_background));
        assert_eq!(palette.ansi, colors.ansi);
    }

    #[test]
    fn terminal_palette_from_theme_preserves_host_extension() {
        let ansi = std::array::from_fn(|i| UiColor::Rgb(i as u8, 10, 20));
        let colors = HostTerminalColors {
            ansi,
            fg: UiColor::Rgb(230, 231, 232),
            bg: UiColor::Rgb(10, 11, 12),
        };
        let theme = Theme::from_host_colors(colors);
        let palette = TerminalColorPalette::from_theme(&theme, UiColor::Rgb(1, 2, 3));

        assert_eq!(palette.foreground, Some(colors.fg));
        assert_eq!(palette.background, Some(UiColor::Rgb(1, 2, 3)));
        assert_eq!(palette.ansi, colors.ansi);
    }

    #[test]
    fn terminal_palette_from_theme_derives_ansi_slots() {
        let foreground = UiColor::Rgb(230, 231, 232);
        let background = UiColor::Rgb(10, 11, 12);
        let accent = UiColor::Rgb(30, 80, 210);
        let theme = Theme::custom(foreground, background, accent);
        let palette = TerminalColorPalette::from_theme(&theme, background);

        assert_eq!(palette.foreground, Some(foreground));
        assert_eq!(palette.background, Some(background));
        assert_eq!(palette.ansi[0], background);
        assert_eq!(palette.ansi[1], theme.status.error);
        assert_eq!(palette.ansi[4], theme.status.info);
        assert_eq!(palette.ansi[12], accent.lighten_by(0.12));
    }

    #[test]
    fn terminal_palette_from_theme_resolves_sentinel_derivations() {
        let mut theme = Theme::custom(UiColor::Backdrop, UiColor::Transparent, UiColor::Reset)
            .primary(Style::new().fg(UiColor::Backdrop))
            .accent(Style::new().fg(UiColor::Transparent))
            .muted(Style::new().fg(UiColor::Reset));
        theme.border_active = UiColor::Backdrop;
        theme.surface.menu = UiColor::Transparent;
        theme.status.error = UiColor::Backdrop;
        theme.status.success = UiColor::Transparent;
        theme.status.warning = UiColor::Reset;
        theme.status.info = UiColor::Backdrop;
        theme.file_icons.purple = UiColor::Transparent;
        theme.file_icons.cyan = UiColor::Reset;

        let palette = TerminalColorPalette::from_theme(&theme, UiColor::Backdrop);
        let colors = palette
            .foreground
            .into_iter()
            .chain(palette.background)
            .chain(palette.ansi)
            .collect::<Vec<_>>();

        assert!(colors.iter().all(|color| !color.is_sentinel()));
        assert_eq!(palette.foreground, Some(UiColor::White));
        assert_eq!(palette.background, Some(UiColor::Black));
        assert_eq!(palette.ansi[1], UiColor::Red);
        assert_eq!(palette.ansi[2], UiColor::Green);
        assert_eq!(palette.ansi[3], UiColor::Yellow);
        assert_eq!(palette.ansi[5], UiColor::Magenta);
        assert_eq!(palette.ansi[6], UiColor::Cyan);
    }

    #[test]
    fn answers_osc_color_queries_from_palette() {
        let mut screen = TerminalScreen::new(2, 8, 10);
        let mut ansi = default_ansi_palette();
        ansi[1] = UiColor::Rgb(0xab, 0xcd, 0xef);
        screen.set_palette(TerminalColorPalette::new(
            UiColor::Rgb(0x11, 0x22, 0x33),
            UiColor::Rgb(0x44, 0x55, 0x66),
            ansi,
        ));

        // Query ANSI slot 1 (OSC 4), default foreground (OSC 10) and background (OSC 11).
        screen.process_bytes(b"\x1b]4;1;?\x1b\\\x1b]10;?\x1b\\\x1b]11;?\x1b\\");
        let responses: Vec<String> = screen
            .drain_responses()
            .into_iter()
            .map(|r| String::from_utf8_lossy(&r).into_owned())
            .collect();

        let joined = responses.join("");
        // Slot 1 reports the themed palette color (8-bit channels doubled to 16-bit).
        assert!(joined.contains("]4;1;rgb:abab/cdcd/efef"), "{joined:?}");
        // OSC 10/11 report the configured default fg/bg.
        assert!(joined.contains("]10;rgb:1111/2222/3333"), "{joined:?}");
        assert!(joined.contains("]11;rgb:4444/5555/6666"), "{joined:?}");
    }

    /// Mode 1016 belongs to the pointer, not the grid, so the emulator underneath never sees it.
    /// A program that asks for it probes whether it took, and answering "not recognized" is what
    /// makes it settle for whole cells - so the screen has to hold the mode and report it itself.
    #[test]
    fn tracks_and_reports_the_childs_request_for_pixel_mouse_reports() {
        let mut screen = TerminalScreen::new(4, 20, 10);
        screen.process_bytes(b"\x1b[?1003h\x1b[?1006h");
        assert_eq!(screen.mouse_mode().encoding, MouseEncoding::Sgr);

        screen.process_bytes(b"\x1b[?1016h\x1b[?1016$p");
        assert_eq!(
            screen.mouse_mode().encoding,
            MouseEncoding::SgrPixels,
            "reports go out in pixels once the child asks"
        );
        let answered = String::from_utf8(screen.drain_responses().concat()).unwrap();
        assert!(answered.contains("\x1b[?1016;1$y"), "{answered:?}");

        screen.process_bytes(b"\x1b[?1016l\x1b[?1016$p");
        assert_eq!(screen.mouse_mode().encoding, MouseEncoding::Sgr);
        let answered = String::from_utf8(screen.drain_responses().concat()).unwrap();
        assert!(answered.contains("\x1b[?1016;2$y"), "{answered:?}");
    }

    #[test]
    fn replay_round_trips_styled_scrollback() {
        let mut screen = TerminalScreen::new(3, 10, 20);
        screen.process_bytes(b"\x1b]2;demo\x1b\\");
        screen.process_bytes(b"\x1b[31mred\x1b[0m\r\n");
        screen.process_bytes(b"\x1b[38;5;45mindexed\x1b[0m\r\n");
        screen.process_bytes(b"\x1b[38;2;1;2;3mtrue\x1b[48;2;4;5;6mcolor\x1b[0m\r\n");
        screen.process_bytes(b"tail");

        let mut target = assert_replay_round_trips(&mut screen);
        assert_scrollback_views_round_trip(&mut screen, &mut target);
    }

    #[test]
    fn replay_soft_wrap_reflows_identically_after_resize() {
        let mut source = TerminalScreen::new(2, 8, 20);
        source.process_bytes(b"abcdefghijklmnopqrst");

        let mut target = assert_replay_round_trips(&mut source);
        assert_scrollback_views_round_trip(&mut source, &mut target);

        source.resize(2, 24);
        target.resize(2, 24);
        let source_snapshot = source.render_snapshot();
        let target_snapshot = target.render_snapshot();

        assert_eq!(target_snapshot.text, source_snapshot.text);
        assert_eq!(target_snapshot.color_lines, source_snapshot.color_lines);
        assert!(source_snapshot.text.starts_with("abcdefghijklmnopqrst"));
    }

    #[test]
    fn replay_round_trips_underline_variants_and_hidden_cells() {
        let mut source = TerminalScreen::new(2, 8, 10);
        source.process_bytes(b"\x1b[4:2mD\x1b[4:3;58;2;1;2;3mC\x1b[4:4mO\x1b[4:5mA\x1b[8mH");

        let target = assert_replay_round_trips(&mut source);
        for (col, flags) in [
            (0, CellFlags::DOUBLE_UNDERLINE),
            (1, CellFlags::UNDERCURL),
            (2, CellFlags::DOTTED_UNDERLINE),
            (3, CellFlags::DASHED_UNDERLINE),
            (4, CellFlags::HIDDEN),
        ] {
            let source_cell = &source.term.grid()[Line(0)][Column(col)];
            let target_cell = &target.term.grid()[Line(0)][Column(col)];
            assert!(source_cell.flags.contains(flags), "source col {col}");
            assert!(target_cell.flags.contains(flags), "target col {col}");
            assert_eq!(target_cell.flags & flags, source_cell.flags & flags);
            assert_eq!(target_cell.underline_color(), source_cell.underline_color());
        }
    }

    #[test]
    fn replay_round_trips_wide_combining_and_modes() {
        let mut screen = TerminalScreen::new(3, 12, 10);
        screen.process_bytes("wide 漢e\u{301}".as_bytes());
        screen.process_bytes(b"\x1b[?25l\x1b[?1003h\x1b[?1006h\x1b[?1004h\x1b[?2004h\x1b[>3u");

        assert_replay_round_trips(&mut screen);
    }

    #[test]
    fn replay_export_is_idempotent() {
        let mut screen = TerminalScreen::new(3, 8, 10);
        screen.process_bytes(b"one\r\ntwo\r\nthree");

        let first = screen.export_replay_bytes();
        let second = screen.export_replay_bytes();

        assert_eq!(first, second);
    }

    #[test]
    fn replay_alt_screen_preserves_source() {
        let mut screen = TerminalScreen::new(3, 10, 10);
        screen.process_bytes(b"primary\r\nline");
        screen.process_bytes(b"\x1b[?1049halt\x1b[32mscreen\x1b[2;3H");
        let before = screen.render_snapshot();
        let before_title = screen.title();

        assert_replay_round_trips(&mut screen);
        let after = screen.render_snapshot();

        assert_eq!(after.text, before.text);
        assert_eq!(after.color_lines, before.color_lines);
        assert_eq!(after.cursor_row, before.cursor_row);
        assert_eq!(after.cursor_col, before.cursor_col);
        assert_eq!(after.cursor_visible, before.cursor_visible);
        assert_eq!(screen.title(), before_title);

        screen.process_bytes(b"Z");
        let after_input = screen.render_snapshot();
        assert!(
            after_input
                .text
                .lines()
                .nth(1)
                .is_some_and(|line| line.starts_with("  Z"))
        );
    }

    #[test]
    fn cursor_defaults_to_blinking_block() {
        let mut screen = TerminalScreen::new(3, 10, 10);
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.cursor_shape, CaretShape::Block);
        assert!(snapshot.cursor_blinking);
    }

    #[test]
    fn key_modes_track_decckm_and_bracketed_paste() {
        let mut screen = TerminalScreen::new(3, 10, 10);
        assert_eq!(screen.key_modes(), TerminalKeyModes::default());

        // DECSET 1 (DECCKM) and DECSET 2004 (bracketed paste), as ncurses' `smkx` and a
        // line editor's paste guard would send them.
        screen.process_bytes(b"\x1b[?1h\x1b[?2004h");
        let modes = screen.render_snapshot().key_modes;
        assert!(modes.app_cursor);
        assert!(modes.bracketed_paste);
        assert_eq!(screen.key_modes(), modes);

        // DECRST puts them back; a child that exits application mode must stop getting SS3.
        screen.process_bytes(b"\x1b[?1l\x1b[?2004l");
        let modes = screen.render_snapshot().key_modes;
        assert!(!modes.app_cursor);
        assert!(!modes.bracketed_paste);
    }

    #[test]
    fn key_modes_track_pushed_kitty_keyboard_flags() {
        let mut screen = TerminalScreen::new(3, 10, 10);
        assert!(!screen.key_modes().kitty_keyboard.any());

        // `CSI > 3 u`: exactly what tui-lipan's own backend pushes on startup
        // (DISAMBIGUATE_ESCAPE_CODES | REPORT_EVENT_TYPES).
        screen.process_bytes(b"\x1b[>3u");
        let flags = screen.render_snapshot().key_modes.kitty_keyboard;
        assert!(flags.disambiguate_escape_codes);
        assert!(flags.report_event_types);
        assert!(!flags.report_alternate_keys);
        assert!(flags.any());

        // `CSI < 1 u` pops the child's push; the encoder must fall back to legacy bytes.
        screen.process_bytes(b"\x1b[<1u");
        assert!(!screen.key_modes().kitty_keyboard.any());
    }

    #[test]
    fn decscusr_sets_cursor_shape_and_blink() {
        let mut screen = TerminalScreen::new(3, 10, 10);

        // CSI 6 SP q: steady bar (odd id blinks, even is steady).
        screen.process_bytes(b"\x1b[6 q");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.cursor_shape, CaretShape::Bar);
        assert!(!snapshot.cursor_blinking);

        // CSI 3 SP q: blinking underline.
        screen.process_bytes(b"\x1b[3 q");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.cursor_shape, CaretShape::Underline);
        assert!(snapshot.cursor_blinking);

        // CSI 2 SP q: steady block.
        screen.process_bytes(b"\x1b[2 q");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.cursor_shape, CaretShape::Block);
        assert!(!snapshot.cursor_blinking);

        // CSI 0 SP q: reset to the configured default (blinking block).
        screen.process_bytes(b"\x1b[0 q");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.cursor_shape, CaretShape::Block);
        assert!(snapshot.cursor_blinking);
    }

    #[test]
    fn export_text_reads_absolute_lines_without_mutating_offset() {
        let mut screen = TerminalScreen::new(3, 10, 20);
        screen.process_bytes(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        screen.set_scrollback(2);
        let offset_before = screen.scrollback_offset();
        let total = screen.total_text_lines();
        assert!(total >= 5);

        let lines = screen.text_lines(0, total);
        assert!(lines.iter().any(|line| line.contains("one")));
        assert!(lines.iter().any(|line| line.contains("five")));
        assert_eq!(screen.scrollback_offset(), offset_before);

        let last_two = screen.export_text(total.saturating_sub(2), total);
        assert!(last_two.contains("four"));
        assert!(last_two.contains("five"));
        assert_eq!(screen.scrollback_offset(), offset_before);
    }

    #[test]
    fn absolute_line_to_viewport_maps_history_and_live_rows() {
        let mut screen = TerminalScreen::new(3, 10, 20);
        screen.process_bytes(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        let total = screen.total_text_lines();

        let (oldest_offset, oldest_row) = screen.absolute_line_to_viewport(0).unwrap();
        assert_eq!(oldest_row, 0);
        assert!(oldest_offset > 0);

        let (live_offset, live_row) = screen
            .absolute_line_to_viewport(total.saturating_sub(1))
            .unwrap();
        assert_eq!(live_offset, 0);
        assert!(live_row < 3);

        assert_eq!(screen.absolute_line_to_viewport(total), None);
    }

    #[test]
    fn export_text_clamps_evicted_and_empty_ranges() {
        let mut screen = TerminalScreen::new(2, 8, 3);
        for i in 0..20 {
            screen.process_bytes(format!("line{i}\r\n").as_bytes());
        }
        let total = screen.total_text_lines();
        assert!(total <= 2 + 3);
        assert!(screen.text_lines(total, total + 10).is_empty());
        assert_eq!(screen.export_text(0, 0), "");
        assert_eq!(screen.text_lines(0, total).len(), total);
    }

    #[test]
    fn text_line_visitor_clamps_ranges_and_stops_immediately() {
        let mut screen = TerminalScreen::new(3, 8, 10);
        screen.process_bytes(b"one\r\ntwo\r\nthree");
        let total = screen.total_text_lines();
        let mut visited = Vec::new();

        let flow = screen.try_for_each_text_line(1, usize::MAX, |absolute, line| {
            visited.push((absolute, line.to_owned()));
            ControlFlow::Break(())
        });
        assert_eq!(flow, ControlFlow::Break(()));
        assert_eq!(visited, [(1, "two".to_string())]);

        let flow = screen.try_for_each_text_line(total + 10, usize::MAX, |_, _| {
            panic!("a fully clamped range must not invoke the visitor")
        });
        assert_eq!(flow, ControlFlow::Continue(()));

        let flow = screen.try_for_each_text_line(2, 1, |_, _| {
            panic!("a reversed half-open range must be empty")
        });
        assert_eq!(flow, ControlFlow::Continue(()));
    }

    #[test]
    fn streaming_text_lines_match_owned_exports_for_special_cells() {
        let mut screen = TerminalScreen::new(5, 20, 0);
        screen.process_bytes("\r\nwide 漢e\u{301}\r\n\r\n".as_bytes());
        screen.process_bytes(b"\x1b[8mH\x1b[0mX");
        let total = screen.total_text_lines();
        let expected = vec![
            String::new(),
            "wide 漢e\u{301}".to_string(),
            String::new(),
            " X".to_string(),
            String::new(),
        ];
        let mut streamed = Vec::new();
        let mut streamed_indices = Vec::new();

        let flow = screen.try_for_each_text_line(0, total, |absolute, line| {
            streamed_indices.push(absolute);
            streamed.push(line.to_owned());
            ControlFlow::Continue(())
        });

        assert_eq!(flow, ControlFlow::Continue(()));
        assert_eq!(streamed_indices, (0..total).collect::<Vec<_>>());
        assert_eq!(streamed, expected);
        assert_eq!(screen.text_lines(0, total), expected);
        assert_eq!(screen.export_text(0, total), "\nwide 漢e\u{301}\n\n X\n");
    }

    #[cfg(feature = "terminal-images")]
    #[test]
    fn streaming_text_lines_match_owned_exports_for_image_placeholders() {
        let mut screen = TerminalScreen::new(1, 8, 0);
        screen.process_bytes(format!("{PLACEHOLDER}X").as_bytes());
        let mut streamed = Vec::new();
        let mut streamed_indices = Vec::new();

        let flow = screen.try_for_each_text_line(0, usize::MAX, |absolute, line| {
            streamed_indices.push(absolute);
            streamed.push(line.to_owned());
            ControlFlow::Continue(())
        });

        assert_eq!(flow, ControlFlow::Continue(()));
        assert_eq!(streamed_indices, [0]);
        assert_eq!(streamed, [" X"]);
        assert_eq!(screen.text_lines(0, usize::MAX), streamed);
        assert_eq!(screen.export_text(0, usize::MAX), " X");
    }

    #[test]
    fn semantic_marks_track_prompt_and_output_ranges() {
        let mut screen = TerminalScreen::new(5, 20, 50);
        // OSC 133 A (prompt), then C (executing), output, then D (completed).
        screen.process_bytes(b"\x1b]133;A\x1b\\");
        screen.process_bytes(b"\x1b]133;C\x1b\\");
        screen.process_bytes(b"hello\r\nworld\r\n");
        screen.process_bytes(b"\x1b]133;D;0\x1b\\");
        screen.process_bytes(b"\x1b]133;A\x1b\\");

        let marks = screen.semantic_marks();
        assert!(
            marks
                .iter()
                .any(|m| m.kind == SemanticMarkKind::OutputStart)
        );
        assert!(
            marks
                .iter()
                .any(|m| { m.kind == SemanticMarkKind::OutputEnd && m.exit_status == Some(0) })
        );
        assert!(marks.iter().any(|m| m.kind == SemanticMarkKind::Prompt));

        let (start, end) = screen.last_command_output_range().expect("range");
        let text = screen.export_text(start, end);
        assert!(text.contains("hello"));
        assert!(text.contains("world"));

        screen.reset();
        assert!(screen.semantic_marks().is_empty());
        assert_eq!(screen.last_command_output_range(), None);
    }

    #[test]
    fn running_command_output_range_extends_to_live_bottom() {
        let mut screen = TerminalScreen::new(4, 20, 20);
        screen.process_bytes(b"\x1b]133;A\x1b\\");
        screen.process_bytes(b"\x1b]133;C\x1b\\");
        screen.process_bytes(b"partial\r\n");
        let (start, end) = screen.last_command_output_range().expect("open range");
        assert!(end > start);
        assert_eq!(end, screen.total_text_lines());
    }

    /// Marks must keep pointing at their own line once scrollback saturates.
    ///
    /// This is the case the grid cannot answer for after the fact: `history_size()`
    /// and `topmost_line()` are pinned while content shifts, so an eviction count
    /// re-derived from the grid is always zero and marks drift onto whatever text
    /// later occupies the index.
    #[test]
    fn marks_survive_eviction_once_scrollback_saturates() {
        let mut screen = TerminalScreen::new(2, 20, 3);
        screen.process_bytes(b"\x1b]133;C\x1b\\");
        screen.process_bytes(b"MARKED\r\n");
        for i in 0..2 {
            screen.process_bytes(format!("filler{i}\r\n").as_bytes());
        }

        // Still retained: the mark must resolve to the line it was recorded on.
        let (start, _) = screen.last_command_output_range().expect("range");
        assert_eq!(
            screen.text_lines(start, start + 1),
            vec!["MARKED".to_string()]
        );

        // Push the marked line out of scrollback entirely; the mark must go with it
        // rather than survive pointing at unrelated text.
        for i in 0..10 {
            screen.process_bytes(format!("more{i}\r\n").as_bytes());
        }
        assert!(
            !screen
                .text_lines(0, screen.total_text_lines())
                .iter()
                .any(|line| line.contains("MARKED")),
            "precondition: the marked line should have been evicted"
        );
        assert_eq!(
            screen.last_command_output_range(),
            None,
            "an evicted mark must be dropped, not left pointing at recycled lines"
        );
    }

    #[test]
    fn alt_screen_marks_do_not_leak_onto_main_screen() {
        let mut screen = TerminalScreen::new(5, 20, 50);
        screen.process_bytes(b"\x1b[?1049h");
        screen.process_bytes(b"\x1b]133;A\x1b\\\x1b]133;C\x1b\\");
        screen.process_bytes(b"altstuff\r\n");
        screen.process_bytes(b"\x1b[?1049l");
        screen.process_bytes(b"back-on-main\r\n");

        assert!(
            screen.semantic_marks().is_empty(),
            "alt-screen OSC 133 must not be replayed against main-screen lines"
        );
        assert_eq!(screen.last_command_output_range(), None);
    }

    #[test]
    fn resize_keeps_scrollback_within_the_requested_limit() {
        let mut screen = TerminalScreen::new(6, 20, 3);
        for i in 0..20 {
            screen.process_bytes(format!("line{i}\r\n").as_bytes());
        }
        // Shrinking rows pushes lines into history outside any handler call.
        screen.resize(2, 20);
        assert!(
            screen.total_scrollback_rows() <= 3,
            "resize must not leave history above the exposed scrollback limit"
        );
    }

    #[test]
    fn reflowing_resize_drops_semantic_marks() {
        let mut screen = TerminalScreen::new(4, 20, 20);
        screen.process_bytes(b"\x1b]133;C\x1b\\");
        screen.process_bytes(b"output\r\n");
        assert!(screen.last_command_output_range().is_some());

        // A column change rewraps history, so recorded line indices become meaningless.
        screen.resize(4, 10);
        assert!(
            screen.semantic_marks().is_empty(),
            "reflow invalidates line anchoring; marks must not survive it"
        );
    }

    #[cfg(feature = "terminal-images")]
    mod images {
        use super::*;
        use crate::widgets::terminal::graphics::TerminalImageCrop;
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;

        /// Transmit-and-display a solid RGB image of `width` x `height` pixels.
        fn place(width: u32, height: u32, keys: &str) -> Vec<u8> {
            let payload = BASE64.encode(vec![0x80u8; (width * height * 3) as usize]);
            format!("\x1b_Ga=T,f=24,s={width},v={height},t=d,{keys};{payload}\x1b\\").into_bytes()
        }

        fn screen(rows: u16, cols: u16, scrollback: usize) -> TerminalScreen {
            let mut screen = TerminalScreen::new(rows, cols, scrollback);
            screen.set_cell_size(TerminalCellSize::new(10, 20));
            screen
        }

        #[test]
        fn graphics_commands_never_reach_the_grid() {
            let mut screen = screen(6, 20, 10);
            let mut stream = b"before".to_vec();
            stream.extend_from_slice(&place(10, 20, "i=1,C=1"));
            stream.extend_from_slice(b"after");
            screen.process_bytes(&stream);

            // The escape is consumed whole: no stray payload characters land in the cells.
            assert!(screen.snapshot().starts_with("beforeafter"));
        }

        #[test]
        fn a_placement_lands_at_the_cursor_and_pushes_it_past_the_image() {
            let mut screen = screen(10, 20, 10);
            screen.process_bytes(b"x");
            // 30x60 pixels in 10x20 cells is 3 columns by 3 rows.
            screen.process_bytes(&place(30, 60, "i=1"));

            let snapshot = screen.render_snapshot();
            assert_eq!(snapshot.images.len(), 1);
            let placement = &snapshot.images[0];
            assert_eq!((placement.row, placement.col), (0, 1));
            assert_eq!((placement.rows, placement.cols), (3, 3));

            // Kitty leaves the cursor on the image's last row, just past its right edge.
            assert_eq!((snapshot.cursor_row, snapshot.cursor_col), (2, 4));
        }

        #[test]
        fn has_images_tracks_the_graphics_store_not_the_viewport() {
            let mut screen = screen(4, 20, 50);
            assert!(!screen.has_images());

            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            assert!(screen.has_images());
            assert!(!screen.render_snapshot().images.is_empty());

            screen.process_bytes(b"\r\n".repeat(10).as_slice());
            assert!(
                screen.render_snapshot().images.is_empty(),
                "scrolled out of the viewport"
            );
            assert!(screen.has_images(), "the store still holds the image");

            screen.process_bytes(b"\x1b_Ga=d,d=A\x1b\\");
            assert!(!screen.has_images());
        }

        #[test]
        fn an_image_scrolls_with_the_text_it_was_drawn_against() {
            let mut screen = screen(6, 20, 50);
            // 20x100 pixels in 10x20 cells is 2 columns by 5 rows.
            screen.process_bytes(&place(20, 100, "i=1,C=1"));
            assert_eq!(screen.render_snapshot().images[0].row, 0);

            // Three lines still fit on a six-row screen, so nothing moves.
            screen.process_bytes(b"\r\n".repeat(3).as_slice());
            assert_eq!(screen.render_snapshot().images[0].row, 0);

            // Past the bottom the grid scrolls, and the image goes up with the text: its top row
            // is now above the viewport, and it reports a negative row so the renderer can crop.
            screen.process_bytes(b"\r\n".repeat(3).as_slice());
            assert_eq!(screen.render_snapshot().images[0].row, -1);
        }

        #[test]
        fn scrolling_back_brings_an_image_back_into_view() {
            let mut screen = screen(4, 20, 50);
            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            screen.process_bytes(b"\r\n".repeat(10).as_slice());
            assert!(screen.render_snapshot().images.is_empty());

            screen.set_scrollback(10);
            let snapshot = screen.render_snapshot();
            assert_eq!(snapshot.images.len(), 1);
            assert_eq!(snapshot.images[0].row, 0);
        }

        #[test]
        fn images_do_not_survive_the_alternate_screen_they_were_drawn_on() {
            let mut screen = screen(6, 20, 10);
            screen.process_bytes(b"\x1b[?1049h");
            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            assert_eq!(screen.render_snapshot().images.len(), 1);

            screen.process_bytes(b"\x1b[?1049l");
            assert!(screen.render_snapshot().images.is_empty());
        }

        #[test]
        fn a_probe_is_answered_on_the_response_channel() {
            let mut screen = screen(6, 20, 10);
            let payload = BASE64.encode([1u8, 2, 3]);
            screen.process_bytes(
                format!("\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;{payload}\x1b\\").as_bytes(),
            );

            let responses = screen.drain_responses();
            assert_eq!(responses.len(), 1);
            assert_eq!(responses[0], b"\x1b_Gi=31;OK\x1b\\");
        }

        #[test]
        fn the_text_area_pixel_size_is_reported_from_the_installed_cell() {
            let mut screen = screen(24, 80, 10);
            screen.process_bytes(b"\x1b[14t");

            let responses = screen.drain_responses();
            // 24 rows of 20px and 80 columns of 10px.
            assert_eq!(responses, vec![b"\x1b[4;480;800t".to_vec()]);
        }

        /// A virtual placement plus the placeholder cells that show it - the shape every terminal
        /// UI toolkit emits, and the one `ratatui_image` writes.
        fn placeholders(id: u32, cols: u16, rows: u16, cell: TerminalCellSize) -> Vec<u8> {
            use crate::widgets::terminal::graphics::diacritic;

            let (width, height) = (
                u32::from(cols) * u32::from(cell.width),
                u32::from(rows) * u32::from(cell.height),
            );
            let payload = BASE64.encode(vec![0x60u8; (width * height * 4) as usize]);
            let mut out = format!(
                "\x1b_Gq=2,i={id},a=T,U=1,f=32,t=d,s={width},v={height},m=0;{payload}\x1b\\"
            )
            .into_bytes();

            let [id_extra, r, g, b] = id.to_be_bytes();
            for row in 0..rows {
                // Absolute placement of each row, the way a full-screen renderer draws.
                out.extend_from_slice(
                    format!("\x1b[{};1H\x1b[38;2;{r};{g};{b}m", row + 1).as_bytes(),
                );
                let mut first = String::from(PLACEHOLDER);
                first.push(diacritic(row));
                first.push(diacritic(0));
                first.push(diacritic(u16::from(id_extra)));
                out.extend_from_slice(first.as_bytes());
                // The rest of the row inherits its position from the cell to its left.
                for _ in 1..cols {
                    out.extend_from_slice(PLACEHOLDER.to_string().as_bytes());
                }
            }
            out
        }

        #[test]
        fn placeholder_cells_place_a_virtual_image() {
            let cell = TerminalCellSize::new(10, 20);
            let mut screen = screen(10, 40, 50);
            screen.process_bytes(&placeholders(1, 6, 3, cell));

            let snapshot = screen.render_snapshot();
            // One placement for the whole picture, not one per row of placeholders.
            assert_eq!(snapshot.images.len(), 1);
            let placement = &snapshot.images[0];
            assert_eq!((placement.row, placement.col), (0, 0));
            assert_eq!((placement.rows, placement.cols), (3, 6));
            assert_eq!(
                placement.source_crop,
                Some(TerminalImageCrop {
                    x: 0,
                    y: 0,
                    width: 60,
                    height: 60,
                })
            );
        }

        #[test]
        fn a_virtual_placement_draws_nothing_where_it_was_transmitted() {
            let mut screen = screen(10, 40, 50);
            // The transmission alone: no placeholders written yet.
            let (width, height) = (60u32, 60u32);
            let payload = BASE64.encode(vec![0x60u8; (width * height * 4) as usize]);
            screen.process_bytes(
                format!("\x1b_Gq=2,i=1,a=T,U=1,f=32,t=d,s={width},v={height},m=0;{payload}\x1b\\")
                    .as_bytes(),
            );

            let snapshot = screen.render_snapshot();
            assert!(
                snapshot.images.is_empty(),
                "a virtual placement must not draw at the cursor"
            );
            // And it must not have moved the cursor either.
            assert_eq!((snapshot.cursor_row, snapshot.cursor_col), (0, 0));
        }

        #[test]
        fn placeholder_cells_are_not_text() {
            let cell = TerminalCellSize::new(10, 20);
            let mut screen = screen(10, 40, 50);
            screen.process_bytes(&placeholders(1, 6, 2, cell));

            let text = screen.snapshot();
            assert!(
                !text.contains(PLACEHOLDER),
                "the placeholder character must not reach text: {text:?}"
            );
        }

        #[test]
        fn scrolling_clips_a_placeholder_image_to_the_cells_still_on_screen() {
            let cell = TerminalCellSize::new(10, 20);
            let mut screen = screen(6, 40, 50);
            screen.process_bytes(&placeholders(1, 4, 2, cell));
            assert_eq!(screen.render_snapshot().images[0].rows, 2);

            // Placeholders live in the grid, so scrolling needs no bookkeeping of our own: the top
            // row leaves the viewport and the placement is simply the row that is left, showing
            // the lower half of the source pixels.
            screen.process_bytes(b"\x1b[6;1H\r\n");
            let snapshot = screen.render_snapshot();
            assert_eq!(snapshot.images.len(), 1);
            let placement = &snapshot.images[0];
            assert_eq!((placement.row, placement.rows), (0, 1));
            assert_eq!(placement.source_crop.unwrap().y, u32::from(cell.height));

            // Scrolling back reaches into history and finds the whole picture again.
            screen.set_scrollback(1);
            assert_eq!(screen.render_snapshot().images[0].rows, 2);
        }

        #[test]
        fn two_images_side_by_side_stay_separate() {
            let cell = TerminalCellSize::new(10, 20);
            let mut screen = screen(10, 40, 50);
            screen.process_bytes(&placeholders(1, 4, 2, cell));
            // A second image, drawn to the right of the first on the same rows.
            let mut second = placeholders(2, 4, 2, cell);
            let shifted = String::from_utf8(second.clone())
                .unwrap()
                .replace("\x1b[1;1H", "\x1b[1;20H")
                .replace("\x1b[2;1H", "\x1b[2;20H");
            second = shifted.into_bytes();
            screen.process_bytes(&second);

            let snapshot = screen.render_snapshot();
            assert_eq!(snapshot.images.len(), 2);
            let cols: Vec<i32> = snapshot.images.iter().map(|image| image.col).collect();
            assert!(cols.contains(&0) && cols.contains(&19), "got {cols:?}");
        }

        /// The regression that made every placeholder image exactly one column wide.
        ///
        /// An id above 24 bits splits between the foreground colour and a third combining mark,
        /// and a sender writes that mark on the first cell of a row only. A continuation cell that
        /// defaults the byte to zero instead of inheriting it names a different image, so every
        /// cell after the first is dropped and the picture collapses to its left edge.
        #[test]
        fn a_continuation_cell_inherits_the_high_byte_of_the_image_id() {
            use crate::widgets::terminal::graphics::diacritic;

            // An id whose top byte is not zero, so the mark actually carries something.
            let id: u32 = 0x02c5_fd02;
            let [id_extra, r, g, b] = id.to_be_bytes();
            assert_ne!(id_extra, 0, "the test is pointless with a small id");

            let mut screen = screen(6, 40, 50);
            let payload = BASE64.encode(vec![0x40u8; (80 * 20 * 4) as usize]);
            screen.process_bytes(
                format!("\x1b_Gq=2,i={id},a=T,U=1,f=32,t=d,s=80,v=20,m=0;{payload}\x1b\\")
                    .as_bytes(),
            );

            let mut row = format!("\x1b[1;1H\x1b[38;2;{r};{g};{b}m");
            row.push(PLACEHOLDER);
            row.push(diacritic(0));
            row.push(diacritic(0));
            row.push(diacritic(u16::from(id_extra)));
            for _ in 1..8 {
                row.push(PLACEHOLDER);
            }
            screen.process_bytes(row.as_bytes());

            let snapshot = screen.render_snapshot();
            assert_eq!(snapshot.images.len(), 1);
            assert_eq!(
                snapshot.images[0].cols, 8,
                "the row collapsed to its first cell"
            );
        }

        #[test]
        fn a_hard_reset_clears_stored_images() {
            let mut screen = screen(6, 20, 10);
            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            assert_eq!(screen.render_snapshot().images.len(), 1);

            screen.reset();
            assert!(screen.render_snapshot().images.is_empty());
        }

        #[test]
        fn a_width_change_that_rewraps_drops_placements() {
            let mut screen = screen(8, 20, 50);
            // A line long enough to wrap, so widening really does redistribute text and the
            // absolute line a placement is anchored to stops naming what it named.
            screen.process_bytes("x".repeat(35).as_bytes());
            screen.process_bytes(b"\r\n");
            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            assert_eq!(screen.render_snapshot().images.len(), 1);

            screen.resize(8, 40);
            assert!(screen.render_snapshot().images.is_empty());
        }

        /// Resizing a pane must not cost it every picture in it. In a tiling multiplexer a width
        /// change is what happens every time a neighbour opens, and treating all of them as a
        /// rewrap made a pane full of plots go blank for it.
        #[test]
        fn a_width_change_that_rewraps_nothing_keeps_placements() {
            let mut screen = screen(8, 40, 50);
            screen.process_bytes(b"short\r\n");
            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            assert_eq!(screen.render_snapshot().images.len(), 1);

            // Nothing on screen is long enough to wrap at either width.
            screen.resize(8, 60);
            assert_eq!(
                screen.render_snapshot().images.len(),
                1,
                "no text moved, so nothing anchored to it should have"
            );
        }

        /// Two placements holding identical pixels must stay distinguishable to the renderer: a
        /// host that draws through Kitty keys a placement by its encoding's id, so sharing one
        /// encoding between them would draw one and silently drop the other.
        #[test]
        fn identical_images_keep_separate_ids() {
            let mut screen = screen(20, 40, 50);
            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            screen.process_bytes(b"\r\n\r\n\r\n");
            screen.process_bytes(&place(20, 40, "i=2,C=1"));

            let snapshot = screen.render_snapshot();
            assert_eq!(snapshot.images.len(), 2);
            assert_eq!(
                snapshot.images[0].image.source_hash(),
                snapshot.images[1].image.source_hash(),
                "the pixels really are identical, which is what makes this worth pinning"
            );
            assert_ne!(
                snapshot.images[0].image_id, snapshot.images[1].image_id,
                "but the placements are not, and the renderer keys on that"
            );
        }
    }

    #[test]
    fn scrollback_depth_matches_requested_limit() {
        let mut screen = TerminalScreen::new(2, 20, 3);
        for i in 0..50 {
            screen.process_bytes(format!("line{i}\r\n").as_bytes());
        }
        // Ledger headroom must not leak into the depth callers observe.
        assert_eq!(screen.total_scrollback_rows(), 3);
        assert_eq!(screen.total_text_lines(), 5);
    }
}
