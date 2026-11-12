use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::ops::ControlFlow;
use std::rc::Rc;
use std::sync::Arc;

use super::damage::{DamageAccumulator, TerminalDamage, TerminalMutation};

mod engine;
mod replay;

#[cfg(test)]
pub(super) use engine::SemanticTestDriver;
use engine::{
    AnchorKind, CellAttrs, CellColor, CellRead, CursorAnchor, Engine, EngineAnchor, EngineConfig,
    EngineCursorShape, EngineCursorStyle, EngineObserver, GridPoint, GridStep, LinkRead, ModeFlags,
    NamedColor, PromptAnchor as ActivePromptMark, ResizeRequest, RowRead, SGR_PIXELS_MOUSE,
    TerminalEngine, TrackedPoint, with_engine,
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
    PromptBoundary, SemanticObserver, TerminalCommandPhase, TerminalSemanticEvent,
    TerminalSemanticState,
};
use super::selection::{ScrollbackLineage, TerminalSelection};
use crate::capture::{
    CapturedCell, CapturedFrame, CellModifiers, CursorShape, CursorState, UnderlineStyle,
};
use crate::style::{CaretShape, Color as UiColor, HostTerminalColors, Rect, Span, Style, Theme};
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
#[non_exhaustive]
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

/// A position in the retained terminal grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalTextPosition {
    /// Absolute retained-line index (`0` = oldest retained grid row).
    pub absolute_line: usize,
    /// Display column within that grid row.
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LogicalTextRow {
    absolute_line: usize,
    byte_start: usize,
    byte_end: usize,
    cell_start: usize,
    cell_end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LogicalTextCell {
    byte_start: usize,
    byte_end: usize,
    column_start: usize,
    column_end: usize,
}

/// A borrowed logical line assembled from one or more soft-wrapped terminal grid rows.
///
/// Values are valid only during the callback passed to
/// [`TerminalScreen::try_for_each_logical_text_line`].
#[derive(Clone, Copy, Debug)]
pub struct TerminalLogicalLine<'a> {
    text: &'a str,
    rows: &'a [LogicalTextRow],
    cells: &'a [LogicalTextCell],
}

impl<'a> TerminalLogicalLine<'a> {
    /// Plain text with soft-wrap boundaries removed.
    pub fn text(self) -> &'a str {
        self.text
    }

    /// Half-open range of absolute grid rows occupied by this logical line.
    pub fn absolute_lines(self) -> std::ops::Range<usize> {
        let start = self.rows.first().map_or(0, |row| row.absolute_line);
        let end = self
            .rows
            .last()
            .map_or(start, |row| row.absolute_line.saturating_add(1));
        start..end
    }

    /// Map an inclusive text-range start byte to its retained grid position.
    ///
    /// At a soft-wrap boundary this chooses the beginning of the continuation row.
    /// A boundary inside a multi-scalar terminal cell chooses the beginning of that cell.
    pub fn start_position(self, byte_index: usize) -> Option<TerminalTextPosition> {
        self.position(byte_index, false)
    }

    /// Map an exclusive text-range end byte to its retained grid position.
    ///
    /// At a soft-wrap boundary this chooses the end of the preceding row.
    /// A boundary inside a multi-scalar terminal cell chooses the end of that cell.
    pub fn end_position(self, byte_index: usize) -> Option<TerminalTextPosition> {
        self.position(byte_index, true)
    }

    fn position(
        self,
        byte_index: usize,
        prefer_preceding_at_boundary: bool,
    ) -> Option<TerminalTextPosition> {
        if byte_index > self.text.len() || !self.text.is_char_boundary(byte_index) {
            return None;
        }
        let row = if prefer_preceding_at_boundary {
            self.rows.iter().find(|row| byte_index <= row.byte_end)
        } else {
            self.rows.iter().rfind(|row| byte_index >= row.byte_start)
        }?;
        let cells = &self.cells[row.cell_start..row.cell_end];
        let column = if byte_index == row.byte_start {
            0
        } else if prefer_preceding_at_boundary {
            cells
                .iter()
                .find(|cell| byte_index <= cell.byte_end)
                .map_or(0, |cell| cell.column_end)
        } else {
            cells
                .iter()
                .find(|cell| byte_index < cell.byte_end)
                .map_or_else(
                    || cells.last().map_or(0, |cell| cell.column_end),
                    |cell| cell.column_start,
                )
        };
        Some(TerminalTextPosition {
            absolute_line: row.absolute_line,
            column,
        })
    }
}

const MAX_SEMANTIC_MARKS: usize = 256;
const MAX_TERMINAL_IDENTITY_LEN: usize = 256;
const DEFAULT_TERMINAL_IDENTITY: &str = concat!("tui-lipan ", env!("CARGO_PKG_VERSION"));

fn normalize_terminal_identity(identity: &str) -> Arc<str> {
    let identity: String = identity
        .chars()
        .filter(char::is_ascii)
        .filter(|character| !character.is_ascii_control())
        .take(MAX_TERMINAL_IDENTITY_LEN)
        .collect();
    let identity = identity.trim();
    if identity.is_empty() {
        Arc::from(DEFAULT_TERMINAL_IDENTITY)
    } else {
        Arc::from(identity)
    }
}

/// Cursor style applied when the child program never issues `DECSCUSR`.
///
/// A blinking block matches the historical default and the common terminal
/// baseline; explicit `CSI Ps SP q` sequences from the child override it.
const DEFAULT_CURSOR_STYLE: EngineCursorStyle = EngineCursorStyle {
    shape: EngineCursorShape::Block,
    blinking: true,
};

/// Map an engine cursor shape to the framework [`CaretShape`].
///
/// `HollowBlock`/`Hidden` collapse to `Block`; visibility is tracked separately
/// via `cursor_visible`.
fn caret_shape_from_engine(shape: EngineCursorShape) -> CaretShape {
    match shape {
        EngineCursorShape::Underline => CaretShape::Underline,
        EngineCursorShape::Beam => CaretShape::Bar,
        EngineCursorShape::Block | EngineCursorShape::HollowBlock | EngineCursorShape::Hidden => {
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

/// Cell size in pixels, as the host draws it.
///
/// A terminal's own contents are cells, but programs that draw pictures need pixels: they read the
/// PTY's `TIOCGWINSZ` pixel fields or ask with `CSI 14 t`, then size their output against the
/// answer. Both are reported from the value installed with
/// [`TerminalScreen::set_cell_size`], so what a child computes and
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

/// Terminal screen parser for PTY output, backed by a VT emulator engine.
pub struct TerminalScreen {
    /// The VT emulator: parser, grids and modes. Everything else here is tui-lipan's own.
    engine: Engine,
    cursor_style_configured: bool,
    /// Replies to the child, in stream order, whether the engine or this screen produced them.
    responses: Vec<Vec<u8>>,
    /// Stable child-facing identity returned for XTVERSION queries.
    terminal_identity: Arc<str>,
    /// Whether XTVERSION queries receive the configured terminal identity.
    xtversion_enabled: bool,
    program_status: super::program_status::ProgramStatusStore,
    program_status_scanner: super::program_status::ProgramStatusScanner,
    drag_and_drop: super::drag_and_drop::DragAndDropCapture,
    /// Semantic/protocol state the engine reports in-band. It cannot affect rendering, and its
    /// state is deliberately not part of `TerminalRenderSnapshot`.
    semantic: SemanticObserver,
    /// Logical viewport rows (matches the PTY size).
    rows: u16,
    /// Logical viewport cols (matches the PTY size).
    cols: u16,
    mouse_mode: MouseModeState,
    scrollback_offset: usize,
    cache: TerminalRenderSnapshot,
    palette: TerminalColorPalette,
    dirty: bool,
    /// Viewport rows changed since the last paint took them.
    damage: DamageAccumulator,
    mutation: TerminalMutation,
    stable_terminal_id: u64,
    #[cfg(feature = "terminal-images")]
    input_revision: u64,
    #[cfg(feature = "terminal-images")]
    prepared_media_commands: VecDeque<GraphicsCommand>,
    presentation_token: Option<u64>,
    mutation_generation: u64,
    cell_generation: u64,
    graphics_generation: u64,
    grid_mutated: bool,
    mutation_waker: Option<std::rc::Weak<dyn Fn()>>,
    #[cfg(feature = "terminal-images")]
    output_transactions: Option<Vec<super::output::TerminalOutputTransaction>>,
    /// A synchronized update held until its end shows whether it is one replacement.
    #[cfg(feature = "terminal-images")]
    output_frame: Option<super::output::OpenFrame>,
    #[cfg(feature = "terminal-images")]
    replacement_run: super::output::ReplacementRun,
    /// Independent from paint damage: a snapshot reader must not consume the renderer's damage.
    snapshot_damage: DamageAccumulator,
    sequence: u64,
    /// Bounded history of OSC 133 marks anchored to absolute text lines.
    semantic_marks: VecDeque<SemanticMark>,
    /// How many semantic events have already been turned into marks (reset on drain).
    semantic_events_seen: usize,
    /// Start of the prompt currently being edited, cleared when command execution begins.
    active_prompt_mark: Option<ActivePromptMark>,
    /// Splits `APC _G` graphics commands out of the byte stream before the grid parser sees it.
    #[cfg(feature = "terminal-images")]
    graphics_scanner: GraphicsScanner,
    /// Decoded images and their placements, anchored to the same absolute lines as the marks.
    #[cfg(feature = "terminal-images")]
    graphics: TerminalGraphics,
    /// Cumulative scrollback lines evicted since creation.
    evicted_lines: u64,
    /// Bumped when absolute line indices are invalidated.
    history_epoch: u64,
    /// Whether the alternate screen was active after the last update.
    alt_screen: bool,
    /// Host cell size, reported to the child and used to size image placements.
    cell_size: TerminalCellSize,
}

struct MutationBaseline {
    cursor: GridPoint,
    cursor_style: EngineCursorStyle,
    pixel_mouse: bool,
    pointer_shape: Option<crate::style::PointerShape>,
    modes: ModeFlags,
    mode_fingerprint: u64,
    title: Option<String>,
    semantic: TerminalSemanticState,
    status: super::ProgramStatusSnapshot,
    clipboard_count: usize,
    response_count: usize,
}

impl MutationBaseline {
    fn cursor_changed(&self, screen: &TerminalScreen) -> bool {
        self.cursor != screen.engine.cursor()
            || self.cursor_style != screen.engine.cursor_style()
            || self.modes.contains(ModeFlags::SHOW_CURSOR)
                != screen.engine.modes().contains(ModeFlags::SHOW_CURSOR)
    }

    fn metadata(&self, screen: &TerminalScreen) -> TerminalMutation {
        let modes = screen.engine.modes();
        TerminalMutation {
            title_changed: self.title != screen.title(),
            modes_changed: self.mode_fingerprint != screen.engine.mode_fingerprint()
                || self.pixel_mouse != modes.contains(ModeFlags::SGR_PIXELS_MOUSE)
                || self.pointer_shape != screen.pointer_shape(),
            semantic_changed: self.semantic != screen.semantic_state()
                || self.status != screen.program_status_snapshot(),
            clipboard_events: self.clipboard_count != screen.engine.clipboard_event_count(),
            responses: self.response_count != screen.responses.len(),
            ..TerminalMutation::default()
        }
    }
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
    /// The screen's columns and rows, or `None` while it is being written to.
    pub(crate) fn size(&self) -> Option<(u16, u16)> {
        self.0
            .try_borrow()
            .ok()
            .map(|screen| (screen.cols, screen.rows))
    }

    /// Current input modes, read directly without constructing a render snapshot.
    pub fn key_modes(&self) -> TerminalKeyModes {
        self.0.borrow().key_modes()
    }

    /// Current mouse pointer, independent of presentation cadence.
    pub fn pointer_shape(&self) -> Option<crate::style::PointerShape> {
        self.0.borrow().pointer_shape()
    }

    /// Current mouse protocol, independent of presentation cadence.
    pub fn mouse_mode(&self) -> MouseModeState {
        self.0.borrow().mouse_mode()
    }

    pub(crate) fn set_mutation_waker(&self, wake: Rc<dyn Fn()>) {
        self.0.borrow_mut().mutation_waker = Some(Rc::downgrade(&wake));
    }

    pub(crate) fn needs_polling(&self) -> bool {
        self.0
            .borrow()
            .mutation_waker
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            .is_none()
    }

    /// Completed parser mutation generation of the shared screen.
    pub fn mutation_generation(&self) -> u64 {
        self.0.borrow().mutation_generation()
    }

    /// Share `screen` with the widget.
    pub fn new(screen: Rc<RefCell<TerminalScreen>>) -> Self {
        Self(screen)
    }

    /// The screen's current snapshot, rebuilding it only if the screen took output since the last
    /// call (see [`TerminalScreen::render_snapshot`]).
    pub fn snapshot(&self) -> TerminalRenderSnapshot {
        self.0.borrow_mut().render_snapshot()
    }

    pub(crate) fn set_host_cursor_style(&self, style: crate::style::TerminalCursorStyle) {
        let mut screen = self.0.borrow_mut();
        if !screen.cursor_style_configured {
            screen.apply_default_cursor_style(style);
        }
    }

    /// Take the viewport rows this screen changed since the last call (see
    /// [`TerminalScreen::take_damage`]).
    ///
    /// Damage is cleared by reading it, so exactly one stage of a frame may call this. That stage
    /// is the live-terminal refresh, which hands the result to whatever else the frame needs it
    /// for.
    pub fn take_damage(&self) -> TerminalDamage {
        self.0.borrow_mut().take_damage()
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
    /// Mouse pointer requested by the child on the active screen.
    pub pointer_shape: Option<crate::style::PointerShape>,
    /// Whether the child program requested a blinking cursor (via `DECSCUSR`).
    pub cursor_blinking: bool,
    /// Application token and screen identity captured with this state.
    pub presentation: Option<super::presentation::PresentedTerminal>,
    /// Stable sequence key for cache invalidation.
    pub sequence: u64,
    /// Generation of cell or cursor changes, independent of graphics revisions.
    pub cell_generation: u64,
    /// Generation of retained graphics changes.
    pub graphics_generation: u64,
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
    /// Explicit OSC 8 hyperlink spans in the visible viewport.
    ///
    /// Each entry covers one contiguous run on one row. A link crossing a soft wrap therefore has
    /// one entry per row, all carrying the same URI.
    pub hyperlinks: Arc<[TerminalHyperlink]>,
}

/// One visible row span carrying an explicit OSC 8 hyperlink.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TerminalHyperlink {
    /// Link destination supplied by the child.
    pub uri: Arc<str>,
    /// Zero-based visible viewport row.
    pub row: usize,
    /// First linked display column, inclusive.
    pub start_col: usize,
    /// Last linked display column, exclusive.
    pub end_col: usize,
}

impl TerminalHyperlink {
    /// Create one explicit hyperlink span.
    pub fn new(uri: impl Into<Arc<str>>, row: usize, cols: std::ops::Range<usize>) -> Self {
        Self {
            uri: uri.into(),
            row,
            start_col: cols.start,
            end_col: cols.end,
        }
    }

    /// Return whether this span contains a visible viewport cell.
    pub fn contains(&self, row: usize, col: usize) -> bool {
        self.row == row && (self.start_col..self.end_col).contains(&col)
    }
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
            pointer_shape: None,
            cursor_blinking: true,
            presentation: None,
            sequence: 0,
            cell_generation: 0,
            graphics_generation: 0,
            scrollback_offset: 0,
            total_scrollback_rows: 0,
            evicted_lines: 0,
            history_epoch: 0,
            mouse_mode: MouseModeState::default(),
            key_modes: TerminalKeyModes::default(),
            #[cfg(feature = "terminal-images")]
            images: Arc::from([]),
            wrapped_rows: Arc::from([]),
            hyperlinks: Arc::from([]),
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

    /// Attach explicit OSC 8 hyperlinks to an externally built snapshot.
    ///
    /// External snapshot transports opt into this metadata just as they do wrap flags. Entries
    /// outside the visible viewport are harmless but can never be activated by the widget.
    pub fn with_hyperlinks(mut self, hyperlinks: impl Into<Arc<[TerminalHyperlink]>>) -> Self {
        self.hyperlinks = hyperlinks.into();
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
            pointer_shape: None,
            cursor_blinking,
            sequence,
            presentation: None,
            cell_generation: sequence,
            graphics_generation: 0,
            scrollback_offset,
            total_scrollback_rows,
            evicted_lines,
            history_epoch,
            mouse_mode,
            key_modes,
            #[cfg(feature = "terminal-images")]
            images: Arc::from([]),
            wrapped_rows: Arc::from([]),
            hyperlinks: Arc::from([]),
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
    /// Terminal default foreground (`SGR 39`).
    pub foreground: Option<UiColor>,
    /// Terminal default background (`SGR 49`).
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
        let engine = Engine::new(EngineConfig {
            rows,
            cols,
            scrollback,
            default_cursor_style: DEFAULT_CURSOR_STYLE,
        });
        let screen = Self {
            damage: DamageAccumulator::default(),
            mutation: TerminalMutation::default(),
            stable_terminal_id: {
                static NEXT_TERMINAL: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(1);
                NEXT_TERMINAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            presentation_token: None,
            mutation_generation: 0,
            cell_generation: 0,
            graphics_generation: 0,
            grid_mutated: false,
            mutation_waker: None,
            #[cfg(feature = "terminal-images")]
            output_transactions: None,
            #[cfg(feature = "terminal-images")]
            output_frame: None,
            #[cfg(feature = "terminal-images")]
            replacement_run: super::output::ReplacementRun::default(),
            snapshot_damage: DamageAccumulator::default(),
            engine,
            cursor_style_configured: false,
            responses: Vec::new(),
            terminal_identity: Arc::from(DEFAULT_TERMINAL_IDENTITY),
            xtversion_enabled: true,
            program_status: Default::default(),
            program_status_scanner: Default::default(),
            drag_and_drop: Default::default(),
            semantic: SemanticObserver::default(),
            rows,
            cols,
            mouse_mode: MouseModeState::default(),
            scrollback_offset: 0,
            cache: TerminalRenderSnapshot::default(),
            palette: TerminalColorPalette::default(),
            dirty: true,
            sequence: 0,
            semantic_marks: VecDeque::new(),
            semantic_events_seen: 0,
            active_prompt_mark: None,
            #[cfg(feature = "terminal-images")]
            graphics_scanner: GraphicsScanner::default(),
            #[cfg(feature = "terminal-images")]
            input_revision: 0,
            #[cfg(feature = "terminal-images")]
            prepared_media_commands: VecDeque::new(),
            #[cfg(feature = "terminal-images")]
            graphics: TerminalGraphics::default(),
            evicted_lines: 0,
            history_epoch: 0,
            alt_screen: false,
            cell_size: TerminalCellSize::default(),
        };
        let mut screen = screen;
        screen.sync_viewport();
        screen
    }

    /// Enable OSC 7501 records and feature replies. Disabled by default for passive parsers.
    ///
    /// Changing enablement discards partial wire sequences. Existing records remain, with
    /// prompt, process-exit and reset lifecycle handling still active while disabled.
    pub fn set_program_status_enabled(&mut self, enabled: bool) {
        if self.program_status.enabled != enabled {
            self.program_status_scanner = Default::default();
            self.program_status.enabled = enabled;
        }
    }

    /// Current terminal-owned records; independent of primary/alternate screen and resize.
    pub fn program_status_snapshot(&self) -> super::ProgramStatusSnapshot {
        self.program_status.snapshot()
    }

    /// Drain effective changes in PTY stream order, including repeated prompts and full resets.
    pub fn drain_program_status_events(&mut self) -> Vec<super::ProgramStatusEvent> {
        self.program_status.drain_events()
    }

    /// Drop active records when the attached process exits, before a replacement PTY starts.
    pub fn program_status_process_exited(&mut self) {
        self.program_status.clear_active();
    }

    /// Speak the drag-and-drop protocol (OSC 72) with the program. Off by default.
    ///
    /// When on, the screen answers the program's support query in stream order and keeps what it
    /// registered ([`Self::drag_and_drop_registration`]). Every other command queues for
    /// [`Self::drain_drag_and_drop_commands`]: the embedder owns the conversation with the host and
    /// answers through the program's input. The registration is forgotten when the shell draws a
    /// prompt (OSC 133 `A`), on a full reset, and when turned off.
    pub fn set_drag_and_drop_enabled(&mut self, enabled: bool) {
        self.drag_and_drop.set_enabled(enabled);
    }

    /// What the program registered for drag and drop.
    pub fn drag_and_drop_registration(&self) -> &crate::dnd::ChildRegistration {
        self.drag_and_drop.registration()
    }

    /// Changes whenever [`Self::drag_and_drop_registration`] does.
    pub fn drag_and_drop_generation(&self) -> u64 {
        self.drag_and_drop.generation()
    }

    /// Take the program's drag-and-drop commands, in stream order, other than its support query
    /// and its registrations.
    pub fn drain_drag_and_drop_commands(&mut self) -> Vec<crate::dnd::TaggedCommand> {
        self.drag_and_drop.drain()
    }

    /// Forget the program's drag-and-drop registration, for an embedder that knows the program
    /// has gone.
    pub fn clear_drag_and_drop_registration(&mut self) {
        self.drag_and_drop.clear_registration();
    }

    /// Feed terminal bytes.
    ///
    pub fn process_bytes(&mut self, bytes: &[u8]) -> TerminalMutation {
        #[cfg(feature = "terminal-images")]
        {
            self.input_revision = self.input_revision.wrapping_add(1);
        }
        self.grid_mutated = false;
        let before = self.mutation_baseline();
        let mut stops = self.chunk_stops(bytes);
        let anchors = self.observing(|engine, observer| engine.scan_ahead(bytes, observer));
        stops.extend(
            anchors
                .into_iter()
                .map(|(end, anchor)| (end, ChunkStop::Anchor(anchor))),
        );
        // Stable: at one offset, this screen's own replies go before the engine's anchor.
        stops.sort_by_key(|(end, _)| *end);
        let evicted = self.feed_grid_with_stops(bytes, stops);
        if evicted > 0 {
            self.evicted_lines = self.evicted_lines.saturating_add(evicted as u64);
        }
        let alt_screen = self.engine.alt_screen();
        if self.alt_screen != alt_screen {
            self.history_epoch = self.history_epoch.saturating_add(1);
            self.alt_screen = alt_screen;
        }
        if alt_screen {
            self.active_prompt_mark = None;
        }
        self.scrollback_offset = self.engine.display_offset();
        self.mouse_mode = mouse_mode_from_modes(self.engine.modes());
        self.finish_mutation(before)
    }

    fn mutation_baseline(&self) -> MutationBaseline {
        let modes = self.engine.modes();
        MutationBaseline {
            cursor: self.engine.cursor(),
            cursor_style: self.engine.cursor_style(),
            pixel_mouse: modes.contains(ModeFlags::SGR_PIXELS_MOUSE),
            pointer_shape: self.pointer_shape(),
            modes,
            mode_fingerprint: self.engine.mode_fingerprint(),
            title: self.title(),
            semantic: self.semantic_state(),
            status: self.program_status_snapshot(),
            clipboard_count: self.engine.clipboard_event_count(),
            response_count: self.responses.len(),
        }
    }

    fn finish_mutation(&mut self, before: MutationBaseline) -> TerminalMutation {
        let mut mutation = before.metadata(self);
        mutation.cursor_changed = before.cursor_changed(self);
        let cells = self.engine.take_damage();
        // Alacritty unconditionally damages the cursor on damage(). Require a completed
        // grid operation or a cursor/mode change before treating it as visual damage.
        let visual_mode = before.mode_fingerprint != self.engine.mode_fingerprint()
            && matches!(cells, TerminalDamage::Full);
        if self.grid_mutated || mutation.cursor_changed || visual_mode {
            mutation.cells = cells;
            self.record_damage(&mutation.cells);
        }
        #[cfg(feature = "terminal-images")]
        {
            mutation.graphics = self.graphics.take_damage();
        }
        if mutation.cells != TerminalDamage::None || mutation.cursor_changed {
            self.cell_generation = self.cell_generation.wrapping_add(1);
        }
        if mutation.graphics != super::damage::GraphicsDamage::None {
            self.graphics_generation = self.graphics_generation.wrapping_add(1);
            // The composed path still needs full cell damage. The native path uses generations.
            self.damage.mark_full();
        }
        self.dirty |= mutation.is_visual() || mutation.modes_changed;
        if !mutation.is_empty() {
            self.mutation.merge(mutation.clone());
            self.notify_mutation();
        }
        crate::core::terminal_metrics::mutation(&mutation);
        mutation
    }

    fn notify_mutation(&mut self) {
        self.mutation_generation = self.mutation_generation.wrapping_add(1);
        if let Some(wake) = self
            .mutation_waker
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
        {
            wake();
        }
    }

    fn invalidate_presentation(&mut self) {
        self.cell_generation = self.cell_generation.wrapping_add(1);
        self.mutation.cells.merge(TerminalDamage::Full);
        self.notify_mutation();
    }

    /// Record lossless output transactions using the existing graphics protocol parser.
    /// Configure this before feeding producer output. Recording is disabled by default, so
    /// ordinary terminal views pay no additional byte-buffering cost.
    #[cfg(feature = "terminal-images")]
    pub fn set_output_transaction_recording(&mut self, enabled: bool) {
        self.output_transactions = enabled.then(Vec::new);
        self.output_frame = None;
        self.replacement_run.clear();
        self.graphics_scanner.set_record_output(enabled);
    }

    /// Take completed transactions in exact producer order. A split APC remains pending until
    /// complete; APCs exceeding the additional 1 MiB buffering bound stream as ordered fragments.
    /// A synchronized update that may be one replacement is held until its end marker.
    #[cfg(feature = "terminal-images")]
    pub fn take_output_transactions(&mut self) -> Vec<super::output::TerminalOutputTransaction> {
        self.output_transactions
            .as_mut()
            .map_or_else(Vec::new, std::mem::take)
    }

    /// Whether output contains an unfinished escape or chunked image transmission. A producer
    /// pacer can use this to grant continuation reads when no completed frame awaits presentation.
    #[cfg(feature = "terminal-images")]
    pub fn output_transaction_pending(&self) -> bool {
        self.graphics_scanner.output_pending()
            || self.graphics.has_pending_transmission()
            || self.output_frame.is_some()
    }

    /// Make pending original bytes ordered before an external resize, reset or exit boundary.
    /// Subsequent continuation bytes remain ordered; the parser itself is not reset.
    #[cfg(feature = "terminal-images")]
    pub fn flush_output_transaction_boundary(&mut self) {
        self.release_output_frame();
        let bytes = self.graphics_scanner.flush_pending_wire();
        // An empty transaction is a failed relay's receipt; an idle boundary has nothing to order.
        if !bytes.is_empty() {
            self.record_owned_output(bytes, super::output::TerminalOutputClass::Ordered);
        }
        self.replacement_run.clear();
    }

    /// Feed text to the grid, holding a synchronized update that may prove to be one replacement.
    #[cfg(feature = "terminal-images")]
    fn feed_text(&mut self, mut bytes: &[u8]) -> usize {
        use super::output::{BEGIN_SYNC, FrameText, OpenFrame, TerminalOutputClass};
        let mut evicted = 0;
        while !bytes.is_empty() {
            if let Some(frame) = self.output_frame.as_mut() {
                let outcome = frame.text(bytes);
                let used = match outcome {
                    FrameText::Open => bytes.len(),
                    FrameText::Ended(used) | FrameText::Unproven(used) => used,
                };
                evicted += self.advance_vte(&bytes[..used]);
                bytes = &bytes[used..];
                match outcome {
                    FrameText::Open => {}
                    FrameText::Ended(_) => self.close_output_frame(),
                    FrameText::Unproven(_) => self.release_output_frame(),
                }
                continue;
            }
            let start = match self.output_transactions {
                Some(_) => bytes
                    .windows(BEGIN_SYNC.len())
                    .position(|window| window == BEGIN_SYNC),
                None => None,
            };
            let Some(start) = start else {
                self.record_output(bytes, TerminalOutputClass::Ordered);
                return evicted + self.advance_vte(bytes);
            };
            let (head, tail) = bytes.split_at(start);
            self.record_output(head, TerminalOutputClass::Ordered);
            evicted += self.advance_vte(head);
            // A begin marker inside an update that is already open does not start a frame.
            let idle = !self.engine.sync_pending() && !self.graphics.has_pending_transmission();
            if idle {
                self.output_frame = Some(OpenFrame::new(self.graphics.evictions()));
            } else {
                self.record_output(BEGIN_SYNC, TerminalOutputClass::Ordered);
            }
            evicted += self.advance_vte(BEGIN_SYNC);
            bytes = &tail[BEGIN_SYNC.len()..];
        }
        evicted
    }

    /// End a held update: one replaceable transaction if proven, otherwise exactly as recorded.
    #[cfg(feature = "terminal-images")]
    fn close_output_frame(&mut self) {
        use super::output::{GraphicsDependency, TerminalOutputClass};
        let Some(frame) = self.output_frame.take() else {
            return;
        };
        let replacement = frame
            .footprint(self.graphics.evictions())
            .and_then(|footprint| {
                let observation = self.graphics.replacement_observation(footprint.image())?;
                Some((observation, footprint))
            });
        let Some((observation, footprint)) = replacement else {
            self.output_frame = Some(frame);
            self.release_output_frame();
            return;
        };
        let class = TerminalOutputClass::Graphics {
            source: observation.source,
            revision: observation.revision,
            dependency: GraphicsDependency::Independent,
        };
        let (bytes, media) = frame.into_output();
        self.push_output(bytes, class, Some(footprint), media);
    }

    /// Stream a held update's transactions unchanged, as if it had never been held.
    #[cfg(feature = "terminal-images")]
    fn release_output_frame(&mut self) {
        let Some(frame) = self.output_frame.take() else {
            return;
        };
        for transaction in frame.into_transactions() {
            self.push_output(
                transaction.bytes,
                transaction.class,
                None,
                transaction.media,
            );
        }
    }

    #[cfg(feature = "terminal-images")]
    fn record_output(&mut self, bytes: &[u8], class: super::output::TerminalOutputClass) {
        if self.output_transactions.is_some() && !bytes.is_empty() {
            for chunk in bytes.chunks(1024 * 1024) {
                self.record_owned_output(chunk.to_vec(), class);
            }
        }
    }

    #[cfg(feature = "terminal-images")]
    fn record_owned_output(&mut self, bytes: Vec<u8>, class: super::output::TerminalOutputClass) {
        self.record_graphics_output(bytes, class, None, Vec::new());
    }

    #[cfg(feature = "terminal-images")]
    fn record_graphics_output(
        &mut self,
        bytes: Vec<u8>,
        class: super::output::TerminalOutputClass,
        footprint: Option<super::output::Footprint>,
        media: Vec<super::output::TerminalOutputMedia>,
    ) {
        use super::output::TerminalOutputClass;
        if bytes.len() > 1024 * 1024 && media.is_empty() {
            self.record_output(&bytes, TerminalOutputClass::Ordered);
            return;
        }
        if self.output_transactions.is_none() {
            return;
        }
        if bytes.is_empty() {
            // This command was deliberately suppressed after a relayed acquisition failed.
            // Preserve a receipt so an embedder cannot mistake no transactions for recording
            // being disabled and fall back to forwarding the original consuming reference.
            self.release_output_frame();
            self.push_output(bytes, TerminalOutputClass::Ordered, None, media);
            return;
        }
        match self.output_frame.as_mut() {
            Some(frame) => {
                frame.push(bytes, class, media);
                if !frame.proven() {
                    self.release_output_frame();
                }
            }
            None => self.push_output(bytes, class, footprint, media),
        }
    }

    /// Append to the recorded list. A replaceable transaction that would share state with a
    /// different source in the current run is downgraded, which also ends the run.
    #[cfg(feature = "terminal-images")]
    fn push_output(
        &mut self,
        bytes: Vec<u8>,
        mut class: super::output::TerminalOutputClass,
        footprint: Option<super::output::Footprint>,
        media: Vec<super::output::TerminalOutputMedia>,
    ) {
        use super::output::{GraphicsDependency, TerminalOutputClass, TerminalOutputTransaction};
        let admitted = match (&class, footprint) {
            (
                TerminalOutputClass::Graphics {
                    source,
                    dependency: GraphicsDependency::Independent,
                    ..
                },
                Some(footprint),
            ) => self.replacement_run.admit(*source, footprint),
            _ => false,
        };
        if !admitted {
            if let TerminalOutputClass::Graphics { dependency, .. } = &mut class
                && *dependency == GraphicsDependency::Independent
            {
                *dependency = GraphicsDependency::DependsOnPrevious;
            }
            self.replacement_run.clear();
        }
        let Some(transactions) = self.output_transactions.as_mut() else {
            return;
        };
        if class == TerminalOutputClass::Ordered
            && let Some(previous) = transactions.last_mut()
            && previous.class == TerminalOutputClass::Ordered
            && previous.bytes.len().saturating_add(bytes.len()) <= 1024 * 1024
        {
            super::output::append_media(&mut previous.media, media, previous.bytes.len());
            previous.bytes.extend_from_slice(&bytes);
        } else {
            transactions.push(TerminalOutputTransaction {
                bytes,
                class,
                media,
            });
        }
    }

    /// Stable identity of this retained screen, independent of mounted widgets or host IDs.
    pub fn stable_terminal_id(&self) -> u64 {
        self.stable_terminal_id
    }

    /// Associate an application sequence with the current retained state.
    /// This does not request a frame or acknowledge presentation.
    pub fn set_presentation_token(&mut self, token: u64) {
        self.presentation_token = Some(token);
        self.cache.presentation = self.presentation();
    }

    fn presentation(&self) -> Option<super::presentation::PresentedTerminal> {
        self.presentation_token
            .map(|token| super::presentation::PresentedTerminal {
                stable_terminal_id: self.stable_terminal_id,
                token,
            })
    }

    /// Take completed parser mutations accumulated since the previous call.
    pub fn take_mutation(&mut self) -> TerminalMutation {
        std::mem::take(&mut self.mutation)
    }

    /// Generation of completed parser changes, independent of render snapshots.
    pub fn mutation_generation(&self) -> u64 {
        self.mutation_generation
    }

    fn record_damage(&mut self, damage: &TerminalDamage) {
        match damage {
            TerminalDamage::Full => {
                self.damage.mark_full();
                self.snapshot_damage.mark_full();
            }
            TerminalDamage::Rows(rows) => {
                for row in rows {
                    self.damage.add_row(
                        usize::from(row.row),
                        usize::from(row.left),
                        usize::from(row.right),
                        usize::from(self.rows),
                    );
                    self.snapshot_damage.add_row(
                        usize::from(row.row),
                        usize::from(row.left),
                        usize::from(row.right),
                        usize::from(self.rows),
                    );
                }
            }
            TerminalDamage::None => {}
        }
    }

    /// Take the rows that changed since the last call, leaving this screen clean.
    pub fn take_damage(&mut self) -> TerminalDamage {
        self.damage.take()
    }

    /// Byte offsets in `bytes` after which a reply this screen produces itself must be queued, so
    /// it keeps its place among the replies the engine produces for the bytes before it.
    fn chunk_stops(&mut self, bytes: &[u8]) -> Vec<(usize, ChunkStop)> {
        let mut stops: Vec<_> = self
            .program_status_scanner
            .scan(bytes, &mut self.program_status)
            .into_iter()
            .map(|end| {
                (
                    end,
                    ChunkStop::Respond(super::program_status::RESPONSE.to_vec()),
                )
            })
            .collect();
        stops.extend(
            self.drag_and_drop
                .scan(bytes)
                .into_iter()
                .map(|(end, reply)| (end, ChunkStop::Respond(reply))),
        );
        stops
    }

    fn feed_grid_with_stops(&mut self, bytes: &[u8], stops: Vec<(usize, ChunkStop)>) -> usize {
        let mut evicted_total = 0;
        let mut start = 0;
        for (end, stop) in stops {
            let end = match stop {
                // An OSC ended by `ESC` is followed by the `\` of `ESC \`. Hold the `ESC` back so
                // the grid parses the pair together: an anchor that commits a synchronized update
                // and reopens it in between would otherwise strand the `\` as text.
                ChunkStop::Anchor(_) if end > start && bytes[end - 1] == 0x1b => end - 1,
                _ => end,
            };
            evicted_total += self.feed_grid(&bytes[start..end]);
            match stop {
                ChunkStop::Respond(response) => self.responses.push(response),
                ChunkStop::Anchor(anchor) => {
                    evicted_total +=
                        self.observed(|engine, observer| engine.anchor(anchor, observer));
                }
            }
            start = end;
        }
        evicted_total + self.feed_grid(&bytes[start..])
    }

    /// Drive the grid parser, returning how many scrollback lines the chunk evicted.
    ///
    /// With image support compiled in, graphics commands are lifted out of the stream first and
    /// the grid sees only what is left, plus whatever cursor movement each placement implies. The
    /// VT parser discards `APC` bodies anyway, so removing them changes nothing it would have
    /// done - what it buys is the cursor position *at* each command, which a parser running
    /// alongside this one could not observe.
    ///
    /// Existing placements are brought in line with the grid before each command, not once per
    /// chunk. A placement anchors to the grid as it stands when the command runs, so lines the
    /// chunk evicted earlier - `clear` erasing history ahead of an image, in one relayed write -
    /// are already behind it, and applying them afterwards would push the new image off the top.
    #[cfg(feature = "terminal-images")]
    fn feed_grid(&mut self, bytes: &[u8]) -> usize {
        if self.graphics_scanner.is_plain(bytes) {
            let evicted = self.feed_text(bytes);
            self.settle_graphics(evicted);
            return evicted;
        }

        let mut evicted = 0;
        let mut unsettled = 0;
        for segment in self.graphics_scanner.scan(bytes) {
            match segment {
                GraphicsSegment::Text(range) => unsettled += self.feed_text(&bytes[range]),
                GraphicsSegment::HeldEscape => unsettled += self.feed_text(&[0x1b]),
                GraphicsSegment::Wire(bytes) => {
                    // Ignored or oversized APC bytes have effects no frame can account for.
                    self.release_output_frame();
                    self.record_owned_output(bytes, super::output::TerminalOutputClass::Ordered)
                }
                GraphicsSegment::Command(mut command) => {
                    if let Some(prepared) = self.prepared_media_commands.pop_front() {
                        command.adopt_preparation(prepared);
                    }
                    if command.is_superseded() {
                        continue;
                    }
                    let (before, after) = self.apply_graphics_with_output(*command, unsettled);
                    evicted += before;
                    unsettled = after;
                }
            }
        }
        self.settle_graphics(unsettled);
        evicted + unsettled
    }

    #[cfg(not(feature = "terminal-images"))]
    fn feed_grid(&mut self, bytes: &[u8]) -> usize {
        self.advance_vte(bytes)
    }

    /// Run one engine operation with this screen observing it in-band, and apply what it did to
    /// the grids. Returns the lines it evicted.
    fn observed(
        &mut self,
        run: impl FnOnce(&mut Engine, &mut FeedObserver<'_>) -> GridStep,
    ) -> usize {
        let step = self.observing(run);
        self.grid_mutated |= step.grid_mutated;
        self.apply_grid_step(step);
        step.evicted
    }

    /// Run one engine operation with this screen observing it in-band.
    fn observing<R>(&mut self, run: impl FnOnce(&mut Engine, &mut FeedObserver<'_>) -> R) -> R {
        let mut observer = FeedObserver {
            semantic: &mut self.semantic,
            marks: &mut self.semantic_marks,
            events_seen: &mut self.semantic_events_seen,
            active_prompt_mark: &mut self.active_prompt_mark,
            responses: &mut self.responses,
            drag_and_drop: &mut self.drag_and_drop,
            history_epoch: &mut self.history_epoch,
            identity: &self.terminal_identity,
            xtversion_enabled: self.xtversion_enabled,
        };
        run(&mut self.engine, &mut observer)
    }

    /// Feed bytes to the engine, returning how many scrollback lines they evicted.
    fn advance_vte(&mut self, bytes: &[u8]) -> usize {
        if bytes.is_empty() {
            return 0;
        }
        self.observed(|engine, observer| engine.feed(bytes, observer))
    }

    #[cfg(feature = "terminal-images")]
    fn apply_graphics_with_output(
        &mut self,
        mut command: GraphicsCommand,
        unsettled: usize,
    ) -> (usize, usize) {
        use super::graphics::FrameOp;
        use super::output::{Footprint, GraphicsDependency, TerminalOutputClass};
        let (media, relay) = self.graphics.media_options();
        if relay.relays(command.medium()) && self.output_transactions.is_some() {
            command.prepare_media(
                media,
                relay,
                true,
                self.graphics.media_files().as_ref(),
                None,
            );
        }
        let original_wire = command.take_wire();
        let Some(wire) = command.take_prepared_wire().or(original_wire) else {
            self.release_output_frame();
            return self.apply_graphics(command, unsettled);
        };
        let op = command.frame_op(relay);
        let source_id = command.transmitted_id();
        let previous = source_id.and_then(|id| self.graphics.replacement_observation(id));
        let evictions = self.graphics.evictions();
        let edit = command.relays_edit();
        let candidate = command.streams_whole_frames(relay)
            && !self.graphics.has_pending_transmission()
            && !self.engine.sync_pending();
        let media = command
            .output_file()
            .map(|file| super::output::TerminalOutputMedia {
                command_range: 0..wire.len(),
                file,
            })
            .into_iter()
            .collect();
        let result = self.apply_graphics(command, unsettled);
        let current = source_id.and_then(|id| self.graphics.replacement_observation(id));
        let class = match current {
            Some(current)
                if previous
                    .as_ref()
                    .is_none_or(|old| old.revision != current.revision) =>
            {
                // Dropping a replacement that evicted would bring the evicted image back.
                let replaces = candidate
                    && self.graphics.evictions() == evictions
                    && previous
                        .as_ref()
                        .is_some_and(|old| current.independently_replaces(old));
                TerminalOutputClass::Graphics {
                    source: current.source,
                    revision: current.revision,
                    dependency: match (replaces, edit) {
                        (true, false) => GraphicsDependency::Independent,
                        (true, true) => GraphicsDependency::EditsPrevious,
                        (false, _) => GraphicsDependency::DependsOnPrevious,
                    },
                }
            }
            _ => TerminalOutputClass::Ordered,
        };
        if let Some(frame) = self.output_frame.as_mut() {
            let succeeded = match op {
                // Only a transmission that stored a new revision is classified as graphics.
                Some(FrameOp::Replace { .. }) => {
                    matches!(class, TerminalOutputClass::Graphics { .. })
                }
                Some(FrameOp::Remove { id }) => self.graphics.replacement_observation(id).is_none(),
                Some(FrameOp::Unplace { .. }) | None => true,
            };
            frame.graphics(op, succeeded);
        }
        let footprint = match op {
            Some(FrameOp::Replace { id, places }) => Some(Footprint::transmission(id, places)),
            _ => None,
        };
        self.record_graphics_output(wire, class, footprint, media);
        result
    }

    /// Apply what the engine saw happen to the grids to the images anchored on them. Line
    /// anchors of the semantic history were already handled in-band by [`FeedObserver`].
    ///
    /// Graphics commands never run inside an engine operation, so an alternate screen it saw left
    /// can only have had placements made before it: they all go with that screen, even when the
    /// same operation opened a fresh one.
    fn apply_grid_step(&mut self, step: GridStep) {
        #[cfg(feature = "terminal-images")]
        {
            // The child hard-reset the terminal (`RIS`), replacing both grids, and images with
            // the grid they were drawn on.
            if step.reset {
                self.graphics.reset();
            }
            if step.left_alt_screen {
                self.graphics.clear_alt_screen();
            }
        }
        #[cfg(not(feature = "terminal-images"))]
        let _ = step;
    }

    /// Run one graphics command against the store, then apply what it implies to the grid.
    ///
    /// `unsettled` is what the chunk evicted since placements were last shifted. Returns the lines
    /// evicted before the command, now settled, and those evicted after it, which the caller still
    /// owes the placements - this one included.
    #[cfg(feature = "terminal-images")]
    fn apply_graphics(&mut self, command: GraphicsCommand, unsettled: usize) -> (usize, usize) {
        // VTE buffers all grid and cursor changes between BSU/ESU synchronized-update markers.
        // A graphics command is intercepted before VTE sees it, so reading the committed grid here
        // would otherwise anchor the placement at the previous frame's cursor. Commit the prefix,
        // apply the command at that cursor, then resume synchronization for the rest of the frame.
        let synchronized = self.engine.sync_pending();
        let mut before = unsettled;
        if synchronized {
            before += self.observed(|engine, observer| engine.commit_sync(observer));
        }
        self.settle_graphics(before);
        let mut after = self.apply_graphics_at_cursor(command);
        if synchronized {
            after += self.observed(|engine, observer| engine.resume_sync(observer));
        }
        (before, after)
    }

    #[cfg(feature = "terminal-images")]
    fn apply_graphics_at_cursor(&mut self, command: GraphicsCommand) -> usize {
        let ctx = GraphicsContext {
            cursor_line: self.engine.cursor_absolute_line(),
            cursor_col: u16::try_from(self.engine.cursor().column).unwrap_or(u16::MAX),
            viewport_top_line: self.engine.history_size(),
            alt_screen: self.engine.alt_screen(),
            cell: self.cell_size,
            cols: self.cols,
            rows: self.rows,
        };
        let outcome = self.graphics.apply(command, ctx);
        if let Some(response) = outcome.response {
            self.responses.push(response);
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
        let history_size = self.engine.history_size();
        let mut images = self
            .graphics
            .visible(history_size, display_offset, self.rows, alt_screen);
        images.extend(self.graphics.placeholder_placements(
            &self.placeholder_cells(display_offset),
            self.cell_size,
            super::graphics::PlaceholderViewport {
                top: history_size as i64 - display_offset as i64
                    + if alt_screen {
                        0
                    } else {
                        self.evicted_lines.min(i64::MAX as u64) as i64
                    },
                oldest: if alt_screen {
                    0
                } else {
                    self.evicted_lines.min(i64::MAX as u64) as i64
                },
                rows: self.rows,
                alternate: alt_screen,
            },
        ));
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

        let (rows, cols) = (self.rows, self.cols);
        with_engine!(&self.engine, engine => {
            push_placeholder_cells(engine, display_offset, rows, cols, &mut cells)
        });
        cells
    }

    /// Shift image placements past the `evicted` lines that fell out of scrollback.
    ///
    /// An alternate screen that was left needs no settling: [`Self::apply_ledger_events`] drops
    /// its placements as the exit happens.
    #[cfg(feature = "terminal-images")]
    fn settle_graphics(&mut self, evicted: usize) {
        self.graphics.drop_evicted(evicted);
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
            self.invalidate_presentation();
            self.snapshot_damage.mark_full();
        }
    }

    /// Set the stable terminal name and version reported to child programs through XTVERSION.
    ///
    /// The response to `CSI > 0 q` is `DCS > | identity ST`. Multiplexers and applications that
    /// embed [`TerminalScreen`] should set their own identity at screen construction time rather
    /// than proxying whichever outer terminal happens to be attached. Control and non-ASCII
    /// characters are removed, the result is capped at 256 bytes, and an empty result restores the
    /// default `tui-lipan <version>` identity.
    pub fn set_terminal_identity(&mut self, identity: impl AsRef<str>) {
        self.terminal_identity = normalize_terminal_identity(identity.as_ref());
    }

    /// The child-facing terminal identity returned by XTVERSION.
    pub fn terminal_identity(&self) -> &str {
        &self.terminal_identity
    }

    /// Enable or disable XTVERSION responses.
    ///
    /// Enabled by default. Disabling this leaves `CSI > 0 q` unanswered while all other terminal
    /// query responses continue normally.
    pub fn set_xtversion_enabled(&mut self, enabled: bool) {
        self.xtversion_enabled = enabled;
    }

    /// Whether XTVERSION queries receive the configured terminal identity.
    pub fn xtversion_enabled(&self) -> bool {
        self.xtversion_enabled
    }

    /// The cell size reported to the child.
    pub fn cell_size(&self) -> TerminalCellSize {
        self.cell_size
    }

    /// Republish the viewport to the engine, which answers pixel-size queries.
    fn sync_viewport(&mut self) {
        self.engine
            .set_report_geometry(self.rows, self.cols, self.cell_size);
    }

    #[cfg(all(test, feature = "terminal-images"))]
    pub(super) fn has_stored_image(&self, id: u32) -> bool {
        self.graphics.replacement_observation(id).is_some()
    }

    /// Cap the decoded pixels this screen retains, in bytes.
    ///
    /// Images past the cap are dropped least-recently-used, placements included. The default is
    /// 96 MiB, which is roughly sixteen 1080p frames.
    #[cfg(feature = "terminal-images")]
    pub fn set_image_budget(&mut self, bytes: usize) {
        self.graphics.set_budget(bytes);
        self.dirty = true;
        self.invalidate_presentation();
        self.snapshot_damage.mark_full();
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
        self.invalidate_presentation();
        self.snapshot_damage.mark_full();
    }

    /// Retain quiet raw file transmissions, and raw out-of-band transmissions prepared with
    /// [`Self::prepare_media_input`], as private immutable file snapshots instead of heap
    /// pixel buffers. Capture, replay and composition read their pixels on demand. A local Kitty
    /// host can load a cell-aligned frame directly, without decoding it in the application.
    ///
    /// Disabled by default. Snapshot failure uses the ordinary in-memory path. File storage is
    /// bounded independently as well as by the screen image budget. Retired storage is reused only
    /// after its last reader releases it and only if it was never handed to a host. Disabling
    /// file storage or resetting/dropping the screen releases its bounded spare allocations.
    #[cfg(feature = "terminal-images")]
    pub fn set_image_file_storage_enabled(&mut self, enabled: bool) {
        self.graphics.set_file_storage_enabled(enabled);
    }

    /// Choose which out-of-band transmission media (`t=f`, `t=t`, `t=s`) this screen accepts.
    ///
    /// These let a child leave a frame in a file or a shared-memory object and name it in a
    /// hundred-byte escape, instead of compressing and base64-ing megabytes of pixels through the
    /// PTY every frame. `t=t` and `t=s` are claimed by one reader. A canonical server may accept
    /// them and use [`super::GraphicsRelayPolicy::NamedAsFiles`] or
    /// [`super::GraphicsRelayPolicy::Consuming`] before fanout. Readers of an
    /// unrelayed stream use [`GraphicsMediaPolicy::SHARED`](crate::widgets::GraphicsMediaPolicy::SHARED),
    /// accepting only re-readable `t=f`.
    #[cfg(feature = "terminal-images")]
    pub fn set_image_media_policy(&mut self, media: super::graphics_media::GraphicsMediaPolicy) {
        self.graphics.set_media_policy(media);
    }

    /// Choose which named sources recorded output replaces, and with what.
    ///
    /// Multiplexer servers use `NamedAsFiles`, with file storage enabled, while every
    /// attachment shares their filesystem, and `All` while any attachment cannot open their files.
    /// `AllWithEdits` relays like `All` but may send a replaced image as root-frame edits of what
    /// changed. Choose it only while every reader applies those edits, and switch back to `All`
    /// when one that does not attaches: such a reader silently keeps the previous frame.
    /// This applies to queries too; failed relayed acquisitions emit no source reference.
    /// Recording must be enabled. The default is `None`. With `NamedAsFiles`, retain the owning
    /// media handles returned by [`Self::take_output_transactions`] until all readers acquire or
    /// discard them. Raw bytes alone have no lifetime guarantee for rewritten file names.
    #[cfg(feature = "terminal-images")]
    pub fn set_image_relay_policy(&mut self, policy: super::GraphicsRelayPolicy) {
        self.graphics.set_relay_policy(policy);
    }

    /// Prepare a named-media input batch for a bounded background worker.
    ///
    /// Returns `None` when ordinary synchronous processing is sufficient. A returned work item
    /// must run and be applied before later input or stream barriers. The scanner clone performs
    /// no I/O and never claims a source. The owner retains ingress/backpressure responsibility.
    #[cfg(feature = "terminal-images")]
    pub fn prepare_media_input(&self, bytes: Arc<[u8]>) -> Option<super::TerminalMediaWork> {
        if self.graphics_scanner.is_plain(&bytes) {
            return None;
        }
        let mut scanner = self.graphics_scanner.clone();
        let (segments, mut length) = scanner.scan_media_prefix(&bytes);
        let mut commands: Vec<_> = segments
            .into_iter()
            .filter_map(|segment| match segment {
                GraphicsSegment::Command(command) => Some(*command),
                _ => None,
            })
            .collect();
        if !commands.iter().any(GraphicsCommand::needs_media) {
            return None;
        }
        let replacements = super::media_work::extend_replacements(
            &mut scanner,
            &bytes,
            &mut commands,
            &mut length,
        );
        let (media, relay) = self.graphics.media_options();
        let recording = self.output_transactions.is_some();
        let base = (recording && relay == super::GraphicsRelayPolicy::AllWithEdits)
            .then(|| super::media_work::batch_relay_base(&self.graphics, &commands, &replacements))
            .flatten();
        Some(super::TerminalMediaWork {
            base,
            replacements,
            bytes,
            length,
            screen_id: self.stable_terminal_id,
            input_revision: self.input_revision,
            commands,
            media,
            relay,
            recording,
            files: self.graphics.media_files(),
        })
    }

    /// Bind already acquired immutable resources to exact complete commands in `bytes`.
    /// Only trusted transport metadata may authorize imports. Unbound input follows ordinary
    /// parsing and copying. The result uses the same screen/revision fence as worker preparation.
    #[cfg(feature = "terminal-images")]
    pub fn prepare_immutable_media_input(
        &self,
        bytes: Arc<[u8]>,
        bindings: Vec<super::output::TerminalOutputMedia>,
    ) -> std::io::Result<super::TerminalPreparedInput> {
        let mut commands = VecDeque::new();
        let mut end = 0;
        for binding in bindings {
            let range = binding.command_range;
            if range.start < end || range.end > bytes.len() || range.start >= range.end {
                return Err(std::io::Error::other("invalid immutable command range"));
            }
            // Parse preceding commands too, so preparation cannot slide onto another command.
            let mut parsed = self.immutable_command_prefix(&bytes, range.clone(), binding.file)?;
            // Preserve previously imported commands, including those without bindings.
            parsed.drain(..commands.len());
            commands.extend(parsed);
            end = range.end;
        }
        // Add any commands after the last binding to preserve parser alignment.
        let mut parsed = self.recorded_media_commands(&bytes);
        parsed.drain(..commands.len());
        commands.extend(parsed);
        let (media, relay) = self.graphics.media_options();
        Ok(super::TerminalPreparedInput {
            length: bytes.len(),
            bytes,
            screen_id: self.stable_terminal_id,
            input_revision: self.input_revision,
            commands,
            media,
            relay,
            recording: self.output_transactions.is_some(),
            files: self.graphics.media_files(),
            replacements: Box::default(),
            base: None,
        })
    }

    #[cfg(feature = "terminal-images")]
    fn recorded_media_commands(&self, bytes: &[u8]) -> Vec<GraphicsCommand> {
        let mut scanner = self.graphics_scanner.clone();
        scanner.set_record_output(true);
        scanner
            .scan(bytes)
            .into_iter()
            .filter_map(|segment| match segment {
                GraphicsSegment::Command(command) => Some(*command),
                _ => None,
            })
            .collect()
    }

    #[cfg(feature = "terminal-images")]
    fn immutable_command_prefix(
        &self,
        bytes: &[u8],
        range: std::ops::Range<usize>,
        file: super::output::TerminalImmutableFile,
    ) -> std::io::Result<Vec<GraphicsCommand>> {
        let mut parsed = self.recorded_media_commands(&bytes[..range.end]);
        let Some(command) = parsed.last_mut() else {
            return Err(std::io::Error::other("immutable binding has no command"));
        };
        if command.wire_bytes() != Some(&bytes[range]) {
            return Err(std::io::Error::other(
                "immutable binding is not an exact command",
            ));
        }
        command.import_file(file)?;
        Ok(parsed)
    }

    /// Commit a worker result through the ordinary parser, retaining output order/classification.
    ///
    /// Returns the unapplied result if it belongs to another screen or later bytes were fed first.
    /// The embedder must serialize resize/reset/policy barriers with this operation as well.
    #[cfg(feature = "terminal-images")]
    pub fn process_prepared_media_input(
        &mut self,
        input: super::TerminalPreparedInput,
    ) -> Result<TerminalMutation, super::TerminalPreparedInput> {
        if input.screen_id != self.stable_terminal_id || input.input_revision != self.input_revision
        {
            return Err(input);
        }
        self.prepared_media_commands = input.commands;
        let mutation = self.process_bytes(&input.bytes[..input.length]);
        debug_assert!(self.prepared_media_commands.is_empty());
        Ok(mutation)
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

        let mut result = String::new();
        with_engine!(&self.engine, engine => {
            for absolute in row_start..=row_end {
                let row = engine.row(absolute);
                let col_start = if absolute == start.line { start.col } else { 0 };
                let col_end = if absolute == end.line {
                    end.col
                        .saturating_add(matches!(endpoint, SelectionEnd::Inclusive) as usize)
                } else {
                    display_line_width(&row)
                };
                let mut text = display_columns_text(&row, col_start, col_end);
                if trim_row_end {
                    text.truncate(text.trim_end().len());
                }
                result.push_str(&text);
                if absolute < row_end {
                    result.push('\n');
                }
            }
        });
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
        std::mem::take(&mut self.responses)
    }

    /// Drain decoded OSC 52 clipboard-store requests emitted by the child.
    ///
    /// Applying these requests is deliberately host policy: an application can ignore them,
    /// write them to a local clipboard, or relay them through an outer terminal. OSC 52 clipboard
    /// loads remain disabled, so a child cannot read clipboard contents through this parser.
    pub fn drain_clipboard_events(&mut self) -> Vec<TerminalClipboardEvent> {
        self.engine.drain_clipboard_events()
    }

    /// Return the number of BEL events received since this screen was created.
    pub fn bell_count(&self) -> u64 {
        self.engine.bell_count()
    }

    /// Serialize the current terminal state as bytes that can be replayed by a
    /// fresh same-sized [`TerminalScreen`].
    ///
    /// The stream captures scrollback, primary/alternate screen contents, retained Kitty images
    /// and placements, the current cursor position/template, title, and common terminal modes. It
    /// is intentionally a replay stream rather than a stable data format: replaying it goes through
    /// the normal VTE parser and future parser fixes naturally apply to exported state.
    ///
    /// Non-goals: tab stops, custom scrolling regions, cursor style, exact Kitty
    /// keyboard stack depth (the effective flags are preserved), and the current
    /// display offset. The receiver lands on the live view.
    pub fn export_replay_bytes(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let result = self.write_replay_bytes(&mut bytes);
        debug_assert!(
            result.is_ok(),
            "writing replay bytes into a Vec cannot fail"
        );
        bytes
    }

    /// Write the current terminal state as replayable bytes without first collecting the complete
    /// stream in memory.
    ///
    /// This has the same contents and source-state guarantees as [`Self::export_replay_bytes`].
    /// The writer may buffer, frame, hash, compress, or persist the stream as it is produced.
    /// Returning an I/O error stops the export; any temporary alternate-screen mutation is still
    /// repaired before the error is returned.
    pub fn write_replay_bytes<W: Write>(&mut self, writer: &mut W) -> io::Result<()> {
        let dirty = self.dirty;
        let cache = self.cache.clone();
        let sequence = self.sequence;
        let scrollback_offset = self.scrollback_offset;
        let mouse_mode = self.mouse_mode;
        let responses = self.drain_responses();

        let was_alt = self.engine.alt_screen();
        let result = self.write_primary_replay(writer).and_then(|()| {
            if !was_alt {
                return Ok(());
            }
            writer.write_all(b"\x1b[?1049h")?;
            self.semantic.pointer.write_stack(writer, true)?;
            let replay = self.replay_context();
            with_engine!(&self.engine, engine => {
                replay.write_active_grid_repaint(engine, writer, false, Some(true))?;
                replay::write_cursor(engine, writer, replay.rows, replay.cols)?;
                write_modes(engine, writer)
            })
        });

        self.responses = responses;
        self.dirty = dirty;
        self.cache = cache;
        self.sequence = sequence;
        self.scrollback_offset = scrollback_offset;
        self.mouse_mode = mouse_mode;
        result
    }

    /// Resize screen dimensions.
    ///
    pub fn resize(&mut self, rows: u16, cols: u16) {
        #[cfg(feature = "terminal-images")]
        {
            self.input_revision = self.input_revision.wrapping_add(1);
        }
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        let reflowed = cols != self.cols;
        self.rows = rows;
        self.cols = cols;
        // Placements follow the text through a rewrap: each anchor cell is tracked by the engine.
        #[cfg(feature = "terminal-images")]
        let track: Option<Vec<TrackedPoint>> = Some(
            self.graphics
                .placement_anchors()
                .into_iter()
                .map(|(absolute_line, column, alt_screen)| TrackedPoint {
                    absolute_line,
                    column,
                    alt_screen,
                })
                .collect(),
        );
        #[cfg(not(feature = "terminal-images"))]
        let track: Option<Vec<TrackedPoint>> = None;
        let outcome = self.engine.resize(ResizeRequest {
            rows,
            cols,
            prompt: self.active_prompt_mark.take(),
            track: track.as_deref(),
        });
        self.active_prompt_mark = outcome.prompt;
        self.sync_viewport();
        let evicted = outcome.evicted;
        if evicted > 0 {
            self.evicted_lines = self.evicted_lines.saturating_add(evicted as u64);
        }
        self.drop_evicted_semantic_marks(evicted);
        if reflowed {
            self.history_epoch = self.history_epoch.saturating_add(1);
            // A column change rewraps history, so line indices no longer refer to the
            // text they were recorded against and cannot be corrected by a shift.
            self.semantic_marks.clear();
            // Images follow the text instead: the engine tracked each anchor cell through the
            // rewrap, and the placement moves to wherever that cell landed. Only when the rewrap
            // actually happened - with nothing moved, the lines they are anchored to still hold.
            #[cfg(feature = "terminal-images")]
            if let Some(relocated) = outcome.relocated {
                self.graphics.relocate_placements(&relocated);
            }
        } else {
            self.settle_graphics(evicted);
        }
        let alt_screen = self.engine.alt_screen();
        if self.alt_screen != alt_screen {
            self.history_epoch = self.history_epoch.saturating_add(1);
            self.alt_screen = alt_screen;
        }
        self.scrollback_offset = self.engine.display_offset();
        self.mouse_mode = mouse_mode_from_modes(self.engine.modes());
        self.dirty = true;
        self.invalidate_presentation();
        self.snapshot_damage.mark_full();
    }

    /// Return current visible screen contents.
    pub fn snapshot(&mut self) -> Arc<str> {
        self.render_snapshot().text
    }

    /// Return full render snapshot (text, colors, cursor).
    pub fn render_snapshot(&mut self) -> TerminalRenderSnapshot {
        if self.dirty {
            self.sequence = self.sequence.saturating_add(1);
            let damage = self.snapshot_damage.take();
            if damage == TerminalDamage::None
                && self.cache.sequence != 0
                && self.cache.cell_generation == self.cell_generation
            {
                #[cfg(feature = "terminal-images")]
                {
                    self.cache.images = self
                        .visible_images(self.engine.display_offset(), self.engine.alt_screen())
                        .into();
                }
                self.cache.mouse_mode = self.mouse_mode;
                self.cache.pointer_shape = self.pointer_shape();
                self.cache.key_modes = self.key_modes();
                self.cache.presentation = self.presentation();
                self.cache.sequence = self.sequence;
                self.cache.graphics_generation = self.graphics_generation;
                self.dirty = false;
                return self.cache.clone();
            }
            let display_offset = self.engine.display_offset();
            let mode = self.engine.modes();
            self.scrollback_offset = display_offset;
            self.mouse_mode = mouse_mode_from_modes(mode);

            let cursor = self.engine.display_cursor();
            let cursor_view = usize::try_from(cursor.line + display_offset as i32).ok();
            let cursor_row = cursor_view.map(|row| row as u16).unwrap_or(0);
            let cursor_col = cursor_view.map(|_| cursor.column as u16).unwrap_or(0);
            let cursor_visible =
                mode.contains(ModeFlags::SHOW_CURSOR) && self.scrollback_offset == 0;
            let cursor_style = self.engine.cursor_style();
            let cursor_shape = caret_shape_from_engine(cursor_style.shape);
            let cursor_blinking = cursor_style.blinking;

            let partial = match &damage {
                TerminalDamage::None
                    if self.cache.color_lines.len() == usize::from(self.rows)
                        && self.cache.scrollback_offset == display_offset =>
                {
                    Some(&[][..])
                }
                TerminalDamage::Rows(rows)
                    if self.cache.color_lines.len() == usize::from(self.rows)
                        && self.cache.scrollback_offset == display_offset =>
                {
                    Some(rows.as_slice())
                }
                _ => None,
            };
            let (rows, cols, palette) = (self.rows, self.cols, self.palette);
            let (mut visible, hyperlinks) = if let Some(damaged) = partial {
                let mut lines = self.cache.color_lines.to_vec();
                for row in damaged {
                    lines[usize::from(row.row)].clear();
                }
                let (lines, mut changed_links) = with_engine!(&self.engine, engine => {
                    renderable_content_lines(
                        engine,
                        damaged.iter().map(|row| usize::from(row.row)),
                        display_offset,
                        rows,
                        cols,
                        palette,
                        lines,
                    )
                });
                changed_links.extend(
                    self.cache
                        .hyperlinks
                        .iter()
                        .filter(|link| !damaged.iter().any(|row| usize::from(row.row) == link.row))
                        .cloned(),
                );
                changed_links.sort_by_key(|link| (link.row, link.start_col));
                (lines, changed_links)
            } else {
                with_engine!(&self.engine, engine => {
                    renderable_content_lines(
                        engine,
                        0..usize::from(rows),
                        display_offset,
                        rows,
                        cols,
                        palette,
                        vec![Vec::new(); usize::from(rows)],
                    )
                })
            };
            if visible.is_empty() {
                visible.push(vec![Span::new("")]);
            }

            #[cfg(feature = "terminal-images")]
            let images = self.visible_images(display_offset, mode.contains(ModeFlags::ALT_SCREEN));

            let wrapped_rows = with_engine!(&self.engine, engine => {
                visible_wrapped_rows(engine, display_offset, rows)
            });

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
                pointer_shape: self.pointer_shape(),
                cursor_blinking,
                presentation: self.presentation(),
                sequence: self.sequence,
                cell_generation: self.cell_generation,
                graphics_generation: self.graphics_generation,
                scrollback_offset: self.scrollback_offset,
                total_scrollback_rows: self.engine.history_size(),
                evicted_lines: self.evicted_lines,
                history_epoch: self.history_epoch,
                mouse_mode: self.mouse_mode,
                key_modes: key_modes_from_modes(mode),
                #[cfg(feature = "terminal-images")]
                images: images.into(),
                wrapped_rows: wrapped_rows.into(),
                hyperlinks: hyperlinks.into(),
            };
            self.dirty = false;
        }
        self.cache.clone()
    }

    /// Capture the visible viewport as a cell grid.
    ///
    /// The frame holds the emulator's own cells, before any widget presentation such as
    /// selection, focus, or decorations. Colors keep their terminal meaning and ignore
    /// [`Self::set_palette`]: default foreground and background stay [`Color::Reset`], the 16 named
    /// ANSI colors stay named, and indexed and truecolor values pass through as written, so
    /// `SGR 38;5;1` captures as `Indexed(1)`, not red. A serializer that needs concrete colors,
    /// such as `CapturedFrame::to_png`, resolves them itself.
    ///
    /// A wide glyph occupies its own cell, and the column it covers holds an empty symbol, so
    /// joining a row's symbols yields text at its true display width. Hidden text captures as
    /// spaces. The cursor is reported while it lies inside the viewport, with the shape and blink
    /// the program asked for through `DECSCUSR` (the screen's configured default otherwise) and
    /// the color it set through `OSC 12`, if any.
    ///
    /// With `terminal-images`, the images the program displayed land in
    /// [`CapturedFrame::images`], cropped to the viewport, and the cells under them hold a
    /// half-block approximation sized with [`Self::cell_size`].
    ///
    /// [`Color::Reset`]: UiColor::Reset
    pub fn capture_frame(&self) -> CapturedFrame {
        let display_offset = self.engine.display_offset();
        let width = self.cols;
        let height = self.rows;
        let blank = CapturedCell {
            symbol: " ".to_string(),
            fg: UiColor::Reset,
            bg: UiColor::Reset,
            underline_color: UiColor::Reset,
            modifiers: CellModifiers::default(),
        };
        let mut cells = vec![blank; usize::from(width) * usize::from(height)];

        with_engine!(&self.engine, engine => {
            for row in 0..usize::from(height) {
                let Some(absolute) = viewport_row_line(engine, display_offset, row) else {
                    continue;
                };
                let grid_row = engine.row(absolute);
                for (col, cell) in grid_row.cells().iter().enumerate().take(usize::from(width)) {
                    cells[row * usize::from(width) + col] = capture_cell(cell);
                }
            }
        });

        let modes = self.engine.modes();
        let style = self.engine.cursor_style();
        let shape = match style.shape {
            EngineCursorShape::Block | EngineCursorShape::Hidden => CursorShape::Block,
            EngineCursorShape::HollowBlock => CursorShape::HollowBlock,
            EngineCursorShape::Underline => CursorShape::Underline,
            EngineCursorShape::Beam => CursorShape::Bar,
        };
        let color = self
            .engine
            .cursor_color()
            .map(|(r, g, b)| UiColor::Rgb(r, g, b));
        let point = self.engine.display_cursor();
        let cursor = usize::try_from(point.line + display_offset as i32)
            .ok()
            .filter(|&line| line < usize::from(height) && point.column < usize::from(width))
            .map(|line| {
                CursorState::new(point.column as u16, line as u16)
                    .visible(
                        modes.contains(ModeFlags::SHOW_CURSOR)
                            && style.shape != EngineCursorShape::Hidden
                            && display_offset == 0,
                    )
                    .shape(shape)
                    .color(color)
                    .blinking(style.blinking)
            });

        #[cfg(feature = "terminal-images")]
        let images = self.capture_images(
            display_offset,
            modes.contains(ModeFlags::ALT_SCREEN),
            &mut cells,
        );
        #[cfg(not(feature = "terminal-images"))]
        let images = Vec::new();

        CapturedFrame {
            viewport: Rect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            },
            width,
            height,
            cells,
            cursor,
            images,
        }
    }

    /// The viewport's images for [`Self::capture_frame`], back to front, with the cells under them
    /// painted as half blocks.
    ///
    /// Each placement is cropped to the viewport the way the renderer crops it, so an image scrolled
    /// half out of view keeps its scale. Where a later image overlaps an earlier one, the earlier one
    /// is marked hidden in those cells.
    #[cfg(feature = "terminal-images")]
    fn capture_images(
        &self,
        display_offset: usize,
        alt_screen: bool,
        cells: &mut [CapturedCell],
    ) -> Vec<crate::capture::CapturedImage> {
        let cols = i32::from(self.cols);
        let rows = i32::from(self.rows);
        let mut images: Vec<crate::capture::CapturedImage> = Vec::new();
        let placements = super::graphics::composite_terminal_images(
            &self.visible_images(display_offset, alt_screen),
            self.cols,
            self.rows,
        );
        for placement in placements {
            let (left, top) = (placement.col, placement.row);
            let (placed_cols, placed_rows) = (i32::from(placement.cols), i32::from(placement.rows));
            if placed_cols <= 0 || placed_rows <= 0 {
                continue;
            }
            let (vis_left, vis_top) = (left.max(0), top.max(0));
            let vis_right = (left + placed_cols).min(cols);
            let vis_bottom = (top + placed_rows).min(rows);
            if vis_right <= vis_left || vis_bottom <= vis_top {
                continue;
            }
            let Some(pixels) = placement.image.pixels() else {
                continue;
            };
            let source = placement.source_crop.unwrap_or(super::TerminalImageCrop {
                x: 0,
                y: 0,
                width: pixels.width(),
                height: pixels.height(),
            });
            let Some(crop) = super::crop_for_visible_cells(
                source,
                (placed_cols as u32, placed_rows as u32),
                (
                    (vis_left - left) as u32,
                    (vis_top - top) as u32,
                    (vis_right - vis_left) as u32,
                    (vis_bottom - vis_top) as u32,
                ),
            ) else {
                continue;
            };
            let cropped = pixels.crop_imm(crop.x, crop.y, crop.width, crop.height);
            let rgba = cropped.to_rgba8();
            let (pixel_width, pixel_height) = rgba.dimensions();
            let mut image = crate::capture::CapturedImage::new(
                Rect {
                    x: vis_left as i16,
                    y: vis_top as i16,
                    w: (vis_right - vis_left) as u16,
                    h: (vis_bottom - vis_top) as u16,
                },
                pixel_width,
                pixel_height,
                rgba.into_raw().into(),
            );
            image.fill_cell_box = true;
            image.z_index = placement.z;
            mask_capture_image(&mut image, cells, self.cols, &cropped, placement.z);
            hide_covered_capture_cells(&mut images, &image, &cropped);
            images.push(image);
        }
        images.retain(|image| image.visible.contains(&true));
        let original_cells = cells.to_vec();
        for image in &mut images {
            image.paint_half_blocks(
                cells,
                self.cols,
                u32::from(self.cell_size.width),
                u32::from(self.cell_size.height),
                &original_cells,
            );
        }
        images
    }

    /// Return the current terminal color palette.
    pub fn palette(&self) -> TerminalColorPalette {
        self.palette
    }

    /// Set the terminal color palette used for future render snapshots.
    pub fn set_palette(&mut self, palette: TerminalColorPalette) {
        if self.palette != palette {
            self.palette = palette;
            // Keep the engine's copy in sync so `OSC 4/10/11 ; ?` color
            // queries are answered against the current palette.
            self.engine.set_palette(&palette);
            self.dirty = true;
            self.invalidate_presentation();
            self.snapshot_damage.mark_full();
        }
    }

    /// Whether the application currently uses the alternate screen.
    pub fn is_alternate_screen(&self) -> bool {
        self.engine.alt_screen()
    }

    /// Observe a history-reader frame without changing the viewport or terminal dimensions.
    pub fn history_read_frame(&mut self) -> super::HistoryReadFrame {
        let synchronized = self.engine.sync_pending();
        let snapshot = self.render_snapshot();
        let mut lines = snapshot.text.split('\n');
        let rows = (0..usize::from(self.rows))
            .map(|index| super::HistoryReadRow {
                text: Arc::from(lines.next().unwrap_or_default()),
                soft_wrapped: snapshot.wrapped_rows.get(index).copied().unwrap_or(false),
            })
            .collect();
        super::HistoryReadFrame {
            viewport: TerminalViewport {
                cols: self.cols,
                rows: self.rows,
            },
            rows,
            synchronized,
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
        self.engine.scroll_display_to(offset);
        self.scrollback_offset = self.engine.display_offset();
        self.dirty = true;
        self.invalidate_presentation();
        self.snapshot_damage.mark_full();
    }

    /// Probe total scrollback rows available.
    pub fn total_scrollback_rows(&mut self) -> usize {
        self.engine.history_size()
    }

    /// Number of plain-text lines currently retained (scrollback history + visible screen).
    ///
    /// Absolute line indices used by [`text_lines`](Self::text_lines) /
    /// [`export_text`](Self::export_text) count from the oldest retained history line (`0`)
    /// through the live bottom (`total_text_lines() - 1`).
    pub fn total_text_lines(&self) -> usize {
        self.engine.total_lines()
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
        visitor: impl FnMut(usize, &str) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        with_engine!(&self.engine, engine => try_for_each_text_line(engine, start, end, visitor))
    }

    /// Visit logical text lines whose first grid row lies in `[start, end)`.
    ///
    /// Unlike [`try_for_each_text_line`](Self::try_for_each_text_line), this joins rows connected by
    /// terminal soft wraps. A continuation already in progress at `start` is skipped, while a line
    /// beginning before `end` is completed even when its continuation extends beyond `end`. These
    /// rules let callers process adjacent physical-row ranges without duplicating or splitting a
    /// logical line.
    ///
    /// The same text and position-metadata scratch allocations are reused for every visit. The
    /// borrowed [`TerminalLogicalLine`] is therefore valid only for that callback invocation.
    pub fn try_for_each_logical_text_line(
        &self,
        start: usize,
        end: usize,
        mut visitor: impl FnMut(TerminalLogicalLine<'_>) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        let total = self.total_text_lines();
        let start = start.min(total);
        let end = end.min(total).max(start);
        with_engine!(&self.engine, engine => {
            let wraps = |absolute: usize| engine.row_wrapped(absolute);
            let mut absolute = start;

            while absolute < end && absolute > 0 && wraps(absolute - 1) {
                absolute += 1;
            }

            let mut text = String::with_capacity(engine.columns());
            let mut rows = Vec::new();
            let mut cells = Vec::with_capacity(engine.columns());
            while absolute < end {
                text.clear();
                rows.clear();
                cells.clear();
                loop {
                    let byte_start = text.len();
                    let cell_start = cells.len();
                    let row = engine.row(absolute);
                    push_plain_line_text_with_cells(&row, &mut text, &mut cells);
                    rows.push(LogicalTextRow {
                        absolute_line: absolute,
                        byte_start,
                        byte_end: text.len(),
                        cell_start,
                        cell_end: cells.len(),
                    });
                    let wrapped = row.wrapped();
                    absolute += 1;
                    if !wrapped || absolute >= total {
                        break;
                    }
                }
                visitor(TerminalLogicalLine {
                    text: &text,
                    rows: &rows,
                    cells: &cells,
                })?;
            }
            ControlFlow::Continue(())
        })
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
        with_engine!(&self.engine, engine => export_text(engine, start, end))
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
        let grid_line = absolute as i32 - self.engine.history_size() as i32;
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
    /// Lines address the primary screen. Uses the last `OutputStart` paired with a following
    /// `OutputEnd`. While a command is still running (start without end), falls back to
    /// `[start, total_text_lines())`.
    ///
    /// While a full-screen program holds the alternate screen, the running command is that
    /// program, and none of what it draws reaches the primary screen. The range is then the most
    /// recent command that completed before it.
    pub fn last_command_output_range(&self) -> Option<(usize, usize)> {
        let alt_screen = self.engine.alt_screen();
        let mut starts = self
            .semantic_marks
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, mark)| mark.kind == SemanticMarkKind::OutputStart);
        if !alt_screen {
            let (start_idx, mark) = starts.next()?;
            let end = self
                .output_end_after(start_idx)
                .unwrap_or_else(|| self.total_text_lines());
            return Some((mark.absolute_line, end.max(mark.absolute_line)));
        }
        starts.find_map(|(start_idx, mark)| {
            let end = self.output_end_after(start_idx)?;
            Some((mark.absolute_line, end.max(mark.absolute_line)))
        })
    }

    fn output_end_after(&self, start_idx: usize) -> Option<usize> {
        self.semantic_marks
            .iter()
            .skip(start_idx + 1)
            .take_while(|mark| mark.kind != SemanticMarkKind::OutputStart)
            .find(|mark| mark.kind == SemanticMarkKind::OutputEnd)
            .map(|mark| mark.absolute_line)
    }

    /// Plain text of [`last_command_output_range`](Self::last_command_output_range), when known.
    ///
    /// Reads the primary screen even while the alternate screen is active.
    pub fn export_last_command_output(&mut self) -> Option<String> {
        let (start, end) = self.last_command_output_range()?;
        Some(with_engine!(&mut self.engine, engine => {
            engine.with_primary_screen(|engine| export_text(engine, start, end))
        }))
    }

    /// Shift marks down by the lines that just fell out of scrollback, dropping those whose line
    /// is gone. See [`drop_evicted_marks`].
    fn drop_evicted_semantic_marks(&mut self, evicted: usize) {
        drop_evicted_marks(
            &mut self.semantic_marks,
            &mut self.active_prompt_mark,
            evicted,
        );
    }

    /// Set the default cursor shape and blink, without replacing explicit child requests.
    ///
    /// Live `Terminal::screen` widgets receive the host's startup style automatically. Apps using
    /// snapshots can call this before parsing output. Headless screens keep a blinking block until
    /// configured. An explicit default takes precedence over host inheritance. `TerminalDefault`
    /// uses a block when no concrete host shape is available.
    pub fn set_default_cursor_style(&mut self, style: crate::style::TerminalCursorStyle) {
        self.cursor_style_configured = true;
        self.apply_default_cursor_style(style);
    }

    fn apply_default_cursor_style(&mut self, style: crate::style::TerminalCursorStyle) {
        let default_cursor_style = EngineCursorStyle {
            shape: match style.shape {
                CaretShape::Bar => EngineCursorShape::Beam,
                CaretShape::Underline => EngineCursorShape::Underline,
                CaretShape::Block | CaretShape::TerminalDefault => EngineCursorShape::Block,
            },
            blinking: style.blinking,
        };
        if !self.engine.set_default_cursor_style(default_cursor_style) {
            return;
        }
        // The engine damages the cursor; the change is accounted for below instead.
        self.engine.take_damage();
        // The caret's appearance is part of what a frame shows, like any cursor change.
        self.cell_generation = self.cell_generation.wrapping_add(1);
        self.dirty = true;
    }

    /// Clear parser state and screen.
    pub fn reset(&mut self) {
        #[cfg(feature = "terminal-images")]
        {
            self.input_revision = self.input_revision.wrapping_add(1);
        }
        // The engine drops any in-flight partial OSC/CSI sequence, its pending replies, title,
        // bell count and clipboard requests.
        self.engine.reset();
        self.responses.clear();
        self.sync_viewport();
        // Keep accumulated semantic state (cwd/command phase/executable) - a child hard-reset
        // (RIS) does not imply the shell's last-known working directory or command lifecycle
        // became invalid.
        self.semantic.pointer = Default::default();
        self.program_status.reset();
        self.program_status_scanner = Default::default();
        self.drag_and_drop.reset();
        self.mouse_mode = MouseModeState::default();
        self.scrollback_offset = 0;
        self.cache = TerminalRenderSnapshot::default();
        self.semantic_marks.clear();
        self.semantic_events_seen = 0;
        self.active_prompt_mark = None;
        self.evicted_lines = 0;
        self.history_epoch = self.history_epoch.saturating_add(1);
        self.alt_screen = false;
        // Images, unlike semantic state, are screen contents: a hard reset clears them with the
        // grid they were drawn on.
        #[cfg(feature = "terminal-images")]
        {
            self.release_output_frame();
            self.replacement_run.clear();
            self.graphics_scanner.reset();
            self.graphics.reset();
        }
        self.dirty = true;
        self.invalidate_presentation();
        self.snapshot_damage.mark_full();
    }

    /// Mouse pointer requested by the child on its active screen, or host default.
    pub fn pointer_shape(&self) -> Option<crate::style::PointerShape> {
        self.semantic.pointer.current()
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
        key_modes_from_modes(self.engine.modes())
    }

    /// The window title the program has set via OSC 0/2 (e.g. the shell's
    /// `$PWD` or a running program's name). Returns `None` if no title has been
    /// set or it was reset. Updated as bytes are processed.
    pub fn title(&self) -> Option<String> {
        self.engine.title()
    }

    fn replay_context(&self) -> ReplayContext<'_> {
        ReplayContext {
            pointer: &self.semantic.pointer,
            #[cfg(feature = "terminal-images")]
            graphics: &self.graphics,
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// Write the primary screen's replay, reading it even while the alternate screen is active.
    fn write_primary_replay<W: Write>(&mut self, writer: &mut W) -> io::Result<()> {
        let replay = ReplayContext {
            pointer: &self.semantic.pointer,
            #[cfg(feature = "terminal-images")]
            graphics: &self.graphics,
            rows: self.rows,
            cols: self.cols,
        };
        with_engine!(&mut self.engine, engine => {
            engine.with_primary_screen(|engine| replay.write_primary_replay(engine, writer))
        })
    }
}

/// What a replay needs from the screen besides the engine.
struct ReplayContext<'a> {
    pointer: &'a super::pointer::PointerState,
    #[cfg(feature = "terminal-images")]
    graphics: &'a TerminalGraphics,
    rows: u16,
    cols: u16,
}

impl ReplayContext<'_> {
    fn write_primary_replay<E: TerminalEngine, W: Write>(
        &self,
        engine: &E,
        writer: &mut W,
    ) -> io::Result<()> {
        writer.write_all(b"\x1bc")?;
        // RIS clears both pointer stacks. Seed the inactive stack before restoring primary
        // contents/cursors/modes, so the temporary switch cannot disturb that restored state.
        writer.write_all(b"\x1b[?1049h")?;
        self.pointer.write_stack(writer, true)?;
        writer.write_all(b"\x1b[?1049l")?;
        #[cfg(feature = "terminal-images")]
        self.graphics.write_replay_transmissions(writer)?;
        self.write_active_grid_repaint(engine, writer, true, Some(false))?;
        replay::write_cursor(engine, writer, self.rows, self.cols)?;
        write_title(engine, writer)?;
        self.pointer.write_stack(writer, false)?;
        write_modes(engine, writer)
    }

    /// Repaint the active grid; `graphics_screen` also restores the image placements of that
    /// screen (`Some(alt_screen)`).
    fn write_active_grid_repaint<E: TerminalEngine, W: Write>(
        &self,
        engine: &E,
        writer: &mut W,
        include_scrollback: bool,
        graphics_screen: Option<bool>,
    ) -> io::Result<()> {
        #[cfg(feature = "terminal-images")]
        let before_line = |writer: &mut W, absolute_line: usize, pending_wrap: bool| {
            let Some(alt_screen) = graphics_screen else {
                return Ok(());
            };
            // A full preceding row has only scheduled an autowrap; the next printable cell
            // normally performs it. Force that transition before a placement's cursor-address
            // command would cancel it, then erase the temporary cell.
            if pending_wrap
                && self
                    .graphics
                    .has_replay_placements(absolute_line, alt_screen)
            {
                writer.write_all(b" \x08\x1b[X")?;
            }
            self.graphics
                .write_replay_placements(writer, absolute_line, alt_screen)
        };
        #[cfg(not(feature = "terminal-images"))]
        let before_line = {
            let _ = graphics_screen;
            |_: &mut W, _: usize, _: bool| Ok(())
        };
        replay::write_grid_repaint(engine, writer, include_scrollback, before_line)
    }
}

fn write_title<E: TerminalEngine, W: Write>(engine: &E, writer: &mut W) -> io::Result<()> {
    if let Some(title) = engine.title().filter(|title| !title.is_empty()) {
        writer.write_all(b"\x1b]2;")?;
        writer.write_all(title.as_bytes())?;
        writer.write_all(b"\x1b\\")?;
    }
    Ok(())
}

fn write_modes<E: TerminalEngine, W: Write>(engine: &E, writer: &mut W) -> io::Result<()> {
    use replay::write_dec_mode;
    let mode = engine.modes();
    write_dec_mode(writer, 1, mode.contains(ModeFlags::APP_CURSOR))?;
    write_dec_mode(writer, 7, mode.contains(ModeFlags::LINE_WRAP))?;
    write_dec_mode(writer, 25, mode.contains(ModeFlags::SHOW_CURSOR))?;
    write_dec_mode(writer, 1000, mode.contains(ModeFlags::MOUSE_REPORT_CLICK))?;
    write_dec_mode(writer, 1002, mode.contains(ModeFlags::MOUSE_DRAG))?;
    write_dec_mode(writer, 1003, mode.contains(ModeFlags::MOUSE_MOTION))?;
    write_dec_mode(writer, 1004, mode.contains(ModeFlags::FOCUS_IN_OUT))?;
    write_dec_mode(writer, 1005, mode.contains(ModeFlags::UTF8_MOUSE))?;
    write_dec_mode(writer, 1006, mode.contains(ModeFlags::SGR_MOUSE))?;
    write_dec_mode(
        writer,
        SGR_PIXELS_MOUSE,
        mode.contains(ModeFlags::SGR_PIXELS_MOUSE),
    )?;
    write_dec_mode(writer, 2004, mode.contains(ModeFlags::BRACKETED_PASTE))?;
    let kitty_flags = u8::from(mode.contains(ModeFlags::DISAMBIGUATE_ESC_CODES))
        | (u8::from(mode.contains(ModeFlags::REPORT_EVENT_TYPES)) << 1)
        | (u8::from(mode.contains(ModeFlags::REPORT_ALTERNATE_KEYS)) << 2)
        | (u8::from(mode.contains(ModeFlags::REPORT_ALL_KEYS_AS_ESC)) << 3)
        | (u8::from(mode.contains(ModeFlags::REPORT_ASSOCIATED_TEXT)) << 4);
    if kitty_flags != 0 {
        write!(writer, "\x1b[>{kitty_flags}u")?;
    }
    writer.write_all(if mode.contains(ModeFlags::APP_KEYPAD) {
        b"\x1b="
    } else {
        b"\x1b>"
    })
}

/// Something to do once the grid has been fed up to a byte offset in a PTY chunk.
enum ChunkStop {
    /// Queue a reply the child asked for, after the bytes before the request.
    Respond(Vec<u8>),
    /// Run an anchor the engine found scanning ahead.
    Anchor(SemanticAnchor),
}

/// What the screen does at the byte where a semantic sequence ended.
struct SemanticAnchor {
    /// `OSC 133;A`/`C`: where the prompt being edited starts, or that it ended.
    boundary: Option<PromptBoundary>,
    /// Reply to an `OSC 22` query.
    pointer_response: Option<Vec<u8>>,
    /// Reply to XTVERSION.
    xtversion_response: Option<Vec<u8>>,
    /// The command phase changed: record marks for the semantic events before this count.
    marks_until: Option<usize>,
}

impl EngineAnchor for SemanticAnchor {
    fn kind(&self) -> AnchorKind {
        if self.boundary.is_some() || self.marks_until.is_some() {
            AnchorKind::Cursor
        } else {
            AnchorKind::Order
        }
    }
}

/// Everything the engine reports in-band while this screen feeds it: replies, semantic sequences
/// and grid steps. Built from the screen's fields other than the engine, so both can be borrowed.
struct FeedObserver<'a> {
    semantic: &'a mut SemanticObserver,
    marks: &'a mut VecDeque<SemanticMark>,
    /// How many semantic events have already been turned into marks.
    events_seen: &'a mut usize,
    active_prompt_mark: &'a mut Option<ActivePromptMark>,
    responses: &'a mut Vec<Vec<u8>>,
    drag_and_drop: &'a mut super::drag_and_drop::DragAndDropCapture,
    history_epoch: &'a mut u64,
    identity: &'a str,
    xtversion_enabled: bool,
}

impl FeedObserver<'_> {
    /// Record marks for the semantic events before `until`, with the grid fed up to them.
    ///
    /// Marks are only recorded on the primary screen. Alt-screen programs still emit OSC 133, but
    /// that grid has no scrollback and no absolute-line space of its own, so those events are
    /// consumed without a mark. A reset earlier in the chunk has already cleared older marks.
    fn record_semantic_marks_until(&mut self, until: usize, cursor: CursorAnchor) {
        let events = self.semantic.peek_events();
        let until = until.min(events.len());
        let from = (*self.events_seen).min(until);
        *self.events_seen = until;
        if cursor.alt_screen {
            return;
        }
        let absolute_line = cursor.absolute_line;
        for event in &events[from..until] {
            let TerminalSemanticEvent::CommandPhaseChanged(phase) = event else {
                continue;
            };
            let mark = match *phase {
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
            self.marks.push_back(mark);
            while self.marks.len() > MAX_SEMANTIC_MARKS {
                self.marks.pop_front();
            }
        }
    }
}

impl EngineObserver for FeedObserver<'_> {
    type Anchor = SemanticAnchor;

    fn respond(&mut self, reply: Vec<u8>) {
        self.responses.push(reply);
    }

    fn grid_step(&mut self, step: GridStep) {
        if step.reset {
            // The child hard-reset the terminal (`RIS`), replacing both grids: drop everything
            // anchored to their lines, as [`TerminalScreen::reset`] does. Accumulated semantic
            // state - the working directory, the command phase - is kept, for the same reason
            // `reset` keeps it.
            self.marks.clear();
            *self.active_prompt_mark = None;
            *self.history_epoch = self.history_epoch.saturating_add(1);
        }
        drop_evicted_marks(self.marks, self.active_prompt_mark, step.evicted);
    }

    fn osc(&mut self, params: &[&[u8]]) {
        self.semantic.osc(params);
    }

    fn xtversion_query(&mut self) {
        self.semantic.xtversion_query();
    }

    fn pointer_reset(&mut self) {
        self.semantic.pointer_reset();
    }

    fn alternate_screen(&mut self, active: bool) {
        self.semantic.alternate_screen(active);
    }

    fn wants_anchor(&self) -> bool {
        self.semantic.wants_cursor() || self.semantic.wants_order()
    }

    fn take_anchor(&mut self) -> Option<SemanticAnchor> {
        if !self.wants_anchor() {
            return None;
        }
        let xtversion = self.semantic.take_xtversion_query() && self.xtversion_enabled;
        let marks_until = self.semantic.take_phase_change().then(|| {
            if self.semantic.state().command_phase == super::TerminalCommandPhase::Prompt {
                self.drag_and_drop.clear_registration();
            }
            self.semantic.event_count()
        });
        Some(SemanticAnchor {
            boundary: self.semantic.take_prompt_boundary(),
            pointer_response: self.semantic.pointer.response.take(),
            xtversion_response: xtversion.then(|| xtversion_response(self.identity)),
            marks_until,
        })
    }

    fn anchor(&mut self, anchor: SemanticAnchor, cursor: Option<CursorAnchor>) {
        // A prompt boundary captures where the prompt starts, so a resize can reflow it.
        if let Some(boundary) = anchor.boundary {
            *self.active_prompt_mark = match (boundary, cursor) {
                (PromptBoundary::Start, Some(cursor)) if !cursor.alt_screen => {
                    Some(ActivePromptMark {
                        absolute_line: cursor.absolute_line,
                        column: cursor.column,
                    })
                }
                _ => None,
            };
        }
        self.responses.extend(anchor.pointer_response);
        self.responses.extend(anchor.xtversion_response);
        if let (Some(until), Some(cursor)) = (anchor.marks_until, cursor) {
            self.record_semantic_marks_until(until, cursor);
        }
    }
}

fn xtversion_response(identity: &str) -> Vec<u8> {
    let mut response = Vec::with_capacity(identity.len() + 7);
    response.extend_from_slice(b"\x1bP>|");
    response.extend_from_slice(identity.as_bytes());
    response.extend_from_slice(b"\x1b\\");
    response
}

/// Shift marks down by the lines that just fell out of scrollback, dropping those whose line is
/// gone.
///
/// `evicted` comes from the engine, which counts evictions as they happen. It cannot be
/// re-derived from the grid afterwards: once scrollback is full, the history size is pinned while
/// content shifts, so a post-hoc comparison sees nothing and marks silently drift onto unrelated
/// lines.
fn drop_evicted_marks(
    marks: &mut VecDeque<SemanticMark>,
    active_prompt_mark: &mut Option<ActivePromptMark>,
    evicted: usize,
) {
    if evicted == 0 {
        return;
    }
    marks.retain(|mark| mark.absolute_line >= evicted);
    for mark in marks.iter_mut() {
        mark.absolute_line -= evicted;
    }
    *active_prompt_mark = active_prompt_mark.and_then(|mut mark| {
        if mark.absolute_line < evicted {
            None
        } else {
            mark.absolute_line -= evicted;
            Some(mark)
        }
    });
}

/// The absolute line shown at viewport `row`, when that row lies inside the retained grid.
fn viewport_row_line<E: TerminalEngine>(
    engine: &E,
    display_offset: usize,
    row: usize,
) -> Option<usize> {
    let line = engine::viewport_line(display_offset, row);
    let history = engine.history_size() as i32;
    (line >= -history && line < engine.screen_lines() as i32)
        .then(|| engine::absolute_line(engine.history_size(), line))
}

/// Visit plain-text grid lines in `[start, end)`; see [`TerminalScreen::try_for_each_text_line`].
fn try_for_each_text_line<E: TerminalEngine>(
    engine: &E,
    start: usize,
    end: usize,
    mut visitor: impl FnMut(usize, &str) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let total = engine.history_size() + engine.screen_lines();
    let start = start.min(total);
    let end = end.min(total).max(start);
    let mut scratch = String::with_capacity(engine.columns());
    for absolute in start..end {
        scratch.clear();
        push_plain_line_text(&engine.row(absolute), &mut scratch);
        visitor(absolute, &scratch)?;
    }
    ControlFlow::Continue(())
}

/// Newline-joined plain text for an absolute line range; see [`TerminalScreen::export_text`].
fn export_text<E: TerminalEngine>(engine: &E, start: usize, end: usize) -> String {
    let mut text = String::new();
    let mut first = true;
    let _ = try_for_each_text_line(engine, start, end, |_, line| {
        if !first {
            text.push('\n');
        }
        first = false;
        text.push_str(line);
        ControlFlow::Continue(())
    });
    text
}

/// Styled spans and hyperlinks for the viewport `rows` given, written into `lines`.
fn renderable_content_lines<E: TerminalEngine>(
    engine: &E,
    rows: impl Iterator<Item = usize>,
    display_offset: usize,
    viewport_rows: u16,
    cols: u16,
    palette: TerminalColorPalette,
    mut lines: Vec<Vec<Span>>,
) -> (Vec<Vec<Span>>, Vec<TerminalHyperlink>) {
    let mut hyperlinks: Vec<TerminalHyperlink> = Vec::new();
    let mut current_row: Option<usize> = None;
    let mut run_style: Option<Style> = None;
    let mut run_key: Option<StyleRunKey> = None;
    let mut run_text = String::new();

    let flush_run = |row: usize,
                     run_style: &mut Option<Style>,
                     run_key: &mut Option<StyleRunKey>,
                     run_text: &mut String,
                     lines: &mut Vec<Vec<Span>>| {
        *run_key = None;
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

    for row in rows {
        if row >= usize::from(viewport_rows) {
            continue;
        }
        let Some(absolute) = viewport_row_line(engine, display_offset, row) else {
            continue;
        };
        if let Some(prev_row) = current_row {
            flush_run(
                prev_row,
                &mut run_style,
                &mut run_key,
                &mut run_text,
                &mut lines,
            );
        }
        current_row = Some(row);

        let grid_row = engine.row(absolute);
        for (col, cell) in grid_row.cells().iter().enumerate().take(usize::from(cols)) {
            if let Some(link) = cell.hyperlink() {
                if let Some(previous) = hyperlinks.last_mut()
                    && previous.row == row
                    && previous.end_col == col
                    && previous.uri.as_ref() == link.uri()
                {
                    previous.end_col = col + 1;
                } else {
                    hyperlinks.push(TerminalHyperlink::new(link.uri(), row, col..col + 1));
                }
            }
            let attrs = cell.attrs();
            if attrs.intersects(CellAttrs::SPACERS) {
                continue;
            }

            let bits = style_run_bits(attrs);
            let (fg, bg) = (cell.fg(), cell.bg());
            match style_run_progress(run_key.as_mut(), fg, bg, bits, &palette) {
                None => {}
                Some((mapped_fg, mapped_bg)) => {
                    flush_run(row, &mut run_style, &mut run_key, &mut run_text, &mut lines);
                    run_style = Some(style_from_mapped(mapped_fg, mapped_bg, bits));
                    run_key = Some(StyleRunKey {
                        fg,
                        bg,
                        mapped_fg,
                        mapped_bg,
                        bits,
                    });
                }
            }
            push_cell_text(&mut run_text, cell, attrs);
        }
    }

    if let Some(row) = current_row {
        flush_run(row, &mut run_style, &mut run_key, &mut run_text, &mut lines);
    }

    for line in &mut lines {
        if line.is_empty() {
            line.push(Span::new(""));
        }
    }

    (lines, hyperlinks)
}

fn push_cell_text_str(out: &mut String, cell: &impl CellRead) {
    push_cell_text(out, cell, cell.attrs());
}

/// [`push_cell_text_str`] with the cell's attributes already read. Inlined into the per-cell
/// snapshot loop, like [`style_run_progress`].
#[inline(always)]
fn push_cell_text(out: &mut String, cell: &impl CellRead, attrs: CellAttrs) {
    let ch = cell.ch();
    // An image placeholder is not text: it names a picture the renderer paints over these cells.
    // Passing it through would put a tofu box under every image, and would put the character into
    // anything that reads the snapshot - a search, a copy, an exported log.
    #[cfg(feature = "terminal-images")]
    if ch == PLACEHOLDER {
        out.push(' ');
        return;
    }
    out.push(if attrs.contains(CellAttrs::HIDDEN) {
        ' '
    } else {
        ch
    });
    for ch in cell.zerowidth() {
        out.push(*ch);
    }
}

/// A viewport cell as [`TerminalScreen::capture_frame`] reports it.
fn capture_cell(cell: &impl CellRead) -> CapturedCell {
    let attrs = cell.attrs();
    let symbol = if attrs.contains(CellAttrs::WIDE_CHAR_SPACER) {
        String::new()
    } else if attrs.contains(CellAttrs::LEADING_WIDE_CHAR_SPACER) {
        " ".to_string()
    } else {
        let mut symbol = String::new();
        push_cell_text_str(&mut symbol, cell);
        symbol
    };
    CapturedCell {
        symbol,
        fg: capture_color(cell.fg()),
        bg: capture_color(cell.bg()),
        underline_color: cell.underline_color().map_or(UiColor::Reset, capture_color),
        modifiers: CellModifiers {
            bold: attrs.contains(CellAttrs::BOLD),
            dim: attrs.contains(CellAttrs::DIM),
            italic: attrs.contains(CellAttrs::ITALIC),
            underline: capture_underline(attrs),
            reverse: attrs.contains(CellAttrs::INVERSE),
            strikethrough: attrs.contains(CellAttrs::STRIKEOUT),
        },
    }
}

/// Soft-wrap flags for the `rows` visible viewport rows at `display_offset`, one per row.
///
/// The terminal records a wrap on the row that overflowed, so the flag reads forward: row `i` is
/// `true` when its text continues on row `i + 1`.
fn visible_wrapped_rows<E: TerminalEngine>(
    engine: &E,
    display_offset: usize,
    rows: u16,
) -> Vec<bool> {
    (0..usize::from(rows))
        .map(|row| {
            viewport_row_line(engine, display_offset, row)
                .is_some_and(|absolute| engine.row_wrapped(absolute))
        })
        .collect()
}

fn display_line_width(row: &impl RowRead) -> usize {
    let cells = row.cells();
    if row.wrapped() {
        return cells.len();
    }
    cells
        .iter()
        .rposition(|cell| !cell.attrs().intersects(CellAttrs::SPACERS) && !cell.is_empty())
        .map_or(0, |col| col + 1)
}

fn display_columns_text(row: &impl RowRead, start_col: usize, end_col: usize) -> String {
    let width = display_line_width(row);
    let col_start = start_col.min(width);
    let col_end = end_col.min(width);
    if col_start >= col_end {
        return String::new();
    }

    let mut result = String::new();
    let mut display_col = 0usize;
    for cell in row.cells() {
        let attrs = cell.attrs();
        if attrs.intersects(CellAttrs::SPACERS) {
            continue;
        }
        let cell_width = if attrs.contains(CellAttrs::WIDE_CHAR) {
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
fn placeholder_id(cell: &impl CellRead) -> Option<u32> {
    match cell.fg() {
        CellColor::Rgb(r, g, b) => Some((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)),
        CellColor::Indexed(index) => Some(u32::from(index)),
        CellColor::Named(_) => None,
    }
}

/// Push the viewport's placeholder cells, left to right and top to bottom.
#[cfg(feature = "terminal-images")]
fn push_placeholder_cells<E: TerminalEngine>(
    engine: &E,
    display_offset: usize,
    rows: u16,
    cols: u16,
    cells: &mut Vec<PlaceholderCell>,
) {
    for row in 0..rows {
        let Some(absolute) = viewport_row_line(engine, display_offset, usize::from(row)) else {
            continue;
        };
        let grid_row = engine.row(absolute);
        for (col, cell) in grid_row.cells().iter().enumerate().take(usize::from(cols)) {
            if cell.ch() != PLACEHOLDER {
                continue;
            }
            let Some(id_low) = placeholder_id(cell) else {
                continue;
            };
            let mut placeholder = PlaceholderCell::new(row, col as u16, id_low, cell.zerowidth());
            placeholder.placement_id = match cell.underline_color() {
                Some(CellColor::Rgb(r, g, b)) => {
                    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
                }
                Some(CellColor::Indexed(index)) => u32::from(index),
                _ => 0,
            };
            cells.push(placeholder);
        }
    }
}

fn push_plain_line_text(row: &impl RowRead, out: &mut String) {
    push_plain_line_text_impl(row, out, |_| {});
}

fn push_plain_line_text_with_cells(
    row: &impl RowRead,
    out: &mut String,
    cells: &mut Vec<LogicalTextCell>,
) {
    push_plain_line_text_impl(row, out, |cell| cells.push(cell));
}

fn push_plain_line_text_impl(
    row: &impl RowRead,
    out: &mut String,
    mut record_cell: impl FnMut(LogicalTextCell),
) {
    let cells = row.cells();
    let end_col = if row.wrapped() {
        cells.len()
    } else {
        cells
            .iter()
            .rposition(|cell| !cell.is_empty())
            .map_or(0, |col| col + 1)
    };
    for (col, cell) in cells[..end_col].iter().enumerate() {
        let attrs = cell.attrs();
        if attrs.intersects(CellAttrs::SPACERS) {
            continue;
        }
        let byte_start = out.len();
        push_cell_text_str(out, cell);
        record_cell(LogicalTextCell {
            byte_start,
            byte_end: out.len(),
            column_start: col,
            column_end: col + usize::from(attrs.contains(CellAttrs::WIDE_CHAR)) + 1,
        });
    }
}

fn key_modes_from_modes(mode: ModeFlags) -> TerminalKeyModes {
    TerminalKeyModes {
        app_cursor: mode.contains(ModeFlags::APP_CURSOR),
        bracketed_paste: mode.contains(ModeFlags::BRACKETED_PASTE),
        kitty_keyboard: KittyKeyboardFlags {
            disambiguate_escape_codes: mode.contains(ModeFlags::DISAMBIGUATE_ESC_CODES),
            report_event_types: mode.contains(ModeFlags::REPORT_EVENT_TYPES),
            report_alternate_keys: mode.contains(ModeFlags::REPORT_ALTERNATE_KEYS),
            report_all_keys_as_escape_codes: mode.contains(ModeFlags::REPORT_ALL_KEYS_AS_ESC),
            report_associated_text: mode.contains(ModeFlags::REPORT_ASSOCIATED_TEXT),
        },
    }
}

fn mouse_mode_from_modes(mode: ModeFlags) -> MouseModeState {
    // 1016 carries SGR's shape, so it supersedes the encoding rather than combining with it, and a
    // program that sets it without 1006 still gets SGR-shaped reports.
    let encoding = if mode.contains(ModeFlags::SGR_PIXELS_MOUSE) {
        MouseEncoding::SgrPixels
    } else if mode.contains(ModeFlags::SGR_MOUSE) {
        MouseEncoding::Sgr
    } else if mode.contains(ModeFlags::UTF8_MOUSE) {
        MouseEncoding::Utf8
    } else {
        MouseEncoding::X10
    };

    let mouse_mode = if mode.contains(ModeFlags::MOUSE_MOTION) {
        MouseMode::AnyEvent
    } else if mode.contains(ModeFlags::MOUSE_DRAG) || mode.contains(ModeFlags::MOUSE_REPORT_CLICK) {
        MouseMode::Normal
    } else {
        MouseMode::None
    };

    let focus_events_enabled = mode.contains(ModeFlags::FOCUS_IN_OUT);

    MouseModeState {
        mode: mouse_mode,
        encoding,
        focus_events_enabled,
    }
}

pub(super) const RUN_BOLD: u8 = 1;
pub(super) const RUN_DIM: u8 = 1 << 1;
pub(super) const RUN_ITALIC: u8 = 1 << 2;
pub(super) const RUN_UNDERLINE: u8 = 1 << 3;
pub(super) const RUN_REVERSE: u8 = 1 << 4;
pub(super) const RUN_STRIKETHROUGH: u8 = 1 << 5;

/// Normalizes the cell attributes that affect `Style`. This is the only place that decides which
/// attributes matter: `style_from_mapped` reads these bits, so equal bits always mean equal flags.
fn style_run_bits(attrs: CellAttrs) -> u8 {
    let bit = |present: bool, bit: u8| if present { bit } else { 0 };
    bit(attrs.contains(CellAttrs::BOLD), RUN_BOLD)
        | bit(attrs.contains(CellAttrs::DIM), RUN_DIM)
        | bit(attrs.contains(CellAttrs::ITALIC), RUN_ITALIC)
        | bit(attrs.intersects(CellAttrs::ALL_UNDERLINES), RUN_UNDERLINE)
        | bit(attrs.contains(CellAttrs::INVERSE), RUN_REVERSE)
        | bit(attrs.contains(CellAttrs::STRIKEOUT), RUN_STRIKETHROUGH)
}

#[derive(Clone, Copy)]
struct StyleRunKey {
    fg: CellColor,
    bg: CellColor,
    mapped_fg: Option<UiColor>,
    mapped_bg: Option<UiColor>,
    bits: u8,
}

/// Returns `None` when a cell with `fg`/`bg` continues the run described by `key`, or the cell's
/// mapped colors when it starts a new run. A continuation through a different color
/// representation that maps to the same colors (`Named(Red)` then `Indexed(1)`) moves the key to
/// that representation, so later cells in the run take the raw-color fast path again.
// Inlined into the per-cell snapshot loop: as a call, its colors cross the boundary packed into
// registers and are unpacked again for every cell, which measured ~9% more instructions per
// snapshot than the code before the engine seam.
#[inline(always)]
fn style_run_progress(
    key: Option<&mut StyleRunKey>,
    fg: CellColor,
    bg: CellColor,
    bits: u8,
    palette: &TerminalColorPalette,
) -> Option<(Option<UiColor>, Option<UiColor>)> {
    let Some(key) = key.filter(|key| key.bits == bits) else {
        return Some((map_term_color(fg, palette), map_term_color(bg, palette)));
    };
    if fg == key.fg && bg == key.bg {
        return None;
    }
    let mapped_fg = if fg == key.fg {
        key.mapped_fg
    } else {
        map_term_color(fg, palette)
    };
    let mapped_bg = if bg == key.bg {
        key.mapped_bg
    } else {
        map_term_color(bg, palette)
    };
    if mapped_fg != key.mapped_fg || mapped_bg != key.mapped_bg {
        return Some((mapped_fg, mapped_bg));
    }
    key.fg = fg;
    key.bg = bg;
    None
}

pub(super) fn style_from_mapped(fg: Option<UiColor>, bg: Option<UiColor>, bits: u8) -> Style {
    Style {
        fg: fg.map(Into::into),
        bg: bg.map(Into::into),
        fg_transform: None,
        bg_transform: None,
        contrast_policy: None,
        bold: Some(bits & RUN_BOLD != 0),
        dim: Some(bits & RUN_DIM != 0),
        italic: Some(bits & RUN_ITALIC != 0),
        underline: Some(bits & RUN_UNDERLINE != 0),
        reverse: Some(bits & RUN_REVERSE != 0),
        dim_amount: None,
        strikethrough: Some(bits & RUN_STRIKETHROUGH != 0),
        underline_color: None,
        tint: None,
    }
}

fn map_term_color(color: CellColor, palette: &TerminalColorPalette) -> Option<UiColor> {
    match color {
        CellColor::Named(named) => map_named_color(named, palette),
        CellColor::Rgb(r, g, b) => Some(UiColor::Rgb(r, g, b)),
        CellColor::Indexed(index) if usize::from(index) < palette.ansi.len() => {
            Some(palette.ansi[usize::from(index)])
        }
        CellColor::Indexed(index) => Some(UiColor::Indexed(index)),
    }
}

/// The underline a cell's attributes ask for. A cell carries at most one underline attribute.
fn capture_underline(attrs: CellAttrs) -> Option<UnderlineStyle> {
    [
        (CellAttrs::UNDERLINE, UnderlineStyle::Single),
        (CellAttrs::DOUBLE_UNDERLINE, UnderlineStyle::Double),
        (CellAttrs::UNDERCURL, UnderlineStyle::Curly),
        (CellAttrs::DOTTED_UNDERLINE, UnderlineStyle::Dotted),
        (CellAttrs::DASHED_UNDERLINE, UnderlineStyle::Dashed),
    ]
    .into_iter()
    .find_map(|(attr, style)| attrs.contains(attr).then_some(style))
}

#[cfg(feature = "terminal-images")]
fn mask_capture_image(
    image: &mut crate::capture::CapturedImage,
    cells: &[CapturedCell],
    cols: u16,
    pixels: &image::DynamicImage,
    z: i32,
) {
    for row in 0..image.area.h {
        for col in 0..image.area.w {
            let x = image.area.x as u16 + col;
            let y = image.area.y as u16 + row;
            let cell = &cells[usize::from(y) * usize::from(cols) + usize::from(x)];
            image.visible[usize::from(row) * usize::from(image.area.w) + usize::from(col)] =
                super::image_covers_cell(z, cell.bg != UiColor::Reset || cell.modifiers.reverse)
                    && super::image_cell_has_pixels(
                        pixels,
                        (image.area.w, image.area.h),
                        (col, row),
                    );
        }
    }
}

#[cfg(feature = "terminal-images")]
fn hide_covered_capture_cells(
    earlier: &mut [crate::capture::CapturedImage],
    image: &crate::capture::CapturedImage,
    pixels: &image::DynamicImage,
) {
    for row in 0..image.area.h {
        for col in 0..image.area.w {
            let x = image.area.x as u16 + col;
            let y = image.area.y as u16 + row;
            if !image.shows(x, y)
                || !super::image_cell_is_opaque(pixels, (image.area.w, image.area.h), (col, row))
            {
                continue;
            }
            for earlier in earlier.iter_mut() {
                if let Some(offset) = earlier.area_offset(x, y) {
                    earlier.visible[offset] = false;
                }
            }
        }
    }
}

/// A terminal color as the program set it, with no palette applied.
///
/// Unlike [`map_term_color`], `SGR 38;5;1` stays `Indexed(1)` rather than becoming the palette's
/// red. The difference is visible: `SGR 58` can only express indexed and RGB colors, so an
/// underline color turned into a named one would be lost when the frame is written back out.
fn capture_color(color: CellColor) -> UiColor {
    match color {
        CellColor::Rgb(r, g, b) => UiColor::Rgb(r, g, b),
        CellColor::Indexed(index) => UiColor::Indexed(index),
        CellColor::Named(named) => match named {
            NamedColor::Black | NamedColor::DimBlack => UiColor::Black,
            NamedColor::Red | NamedColor::DimRed => UiColor::Red,
            NamedColor::Green | NamedColor::DimGreen => UiColor::Green,
            NamedColor::Yellow | NamedColor::DimYellow => UiColor::Yellow,
            NamedColor::Blue | NamedColor::DimBlue => UiColor::Blue,
            NamedColor::Magenta | NamedColor::DimMagenta => UiColor::Magenta,
            NamedColor::Cyan | NamedColor::DimCyan => UiColor::Cyan,
            NamedColor::White | NamedColor::DimWhite => UiColor::Gray,
            NamedColor::BrightBlack => UiColor::DarkGray,
            NamedColor::BrightRed => UiColor::LightRed,
            NamedColor::BrightGreen => UiColor::LightGreen,
            NamedColor::BrightYellow => UiColor::LightYellow,
            NamedColor::BrightBlue => UiColor::LightBlue,
            NamedColor::BrightMagenta => UiColor::LightMagenta,
            NamedColor::BrightCyan => UiColor::LightCyan,
            NamedColor::BrightWhite => UiColor::White,
            NamedColor::Foreground
            | NamedColor::BrightForeground
            | NamedColor::DimForeground
            | NamedColor::Background
            | NamedColor::Cursor => UiColor::Reset,
        },
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
    // These tests inspect the Alacritty engine's grid directly.
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line};

    /// `RIS` replaces the grid the marks were recorded on, including the live screen, so marks
    /// still on screen have to go too; the shell's accumulated state stays.
    #[test]
    fn a_hard_reset_drops_shell_marks_still_on_screen() {
        let mut screen = TerminalScreen::new(8, 30, 100);
        for chunk in [
            "\x1b]7;file://host/tmp/work\x1b\\",
            "\x1b]133;A\x1b\\$ ",
            "\x1b]133;B\x1b\\run\r\n",
            "\x1b]133;C\x1b\\",
            "output\r\n",
        ] {
            screen.process_bytes(chunk.as_bytes());
        }
        assert!(!screen.semantic_marks().is_empty());
        let before = screen.semantic_state();

        screen.process_bytes(b"\x1bc");

        assert!(
            screen.semantic_marks().is_empty(),
            "{:?}",
            screen.semantic_marks()
        );
        assert!(screen.last_command_output_range().is_none());
        assert_eq!(
            screen.semantic_state(),
            before,
            "semantic state is kept, as reset() keeps it"
        );
    }

    /// The semantic parser reads a whole chunk before the grid resets, so a reset inside one chunk
    /// must still split its events: those before it named erased lines, those after it are new.
    /// PTY reads coalesce writes arbitrarily, so one chunk and several must end up the same.
    #[test]
    fn a_hard_reset_splits_the_shell_marks_of_a_chunk_that_contains_it() {
        let kinds = |screen: &TerminalScreen| {
            screen
                .semantic_marks()
                .iter()
                .map(|mark| mark.kind)
                .collect::<Vec<_>>()
        };

        let mut before = TerminalScreen::new(6, 30, 100);
        before.process_bytes(b"\x1b]133;C\x1b\\output\r\n\x1bc");
        assert!(
            before.semantic_marks().is_empty(),
            "{:?}",
            before.semantic_marks()
        );

        let mut after = TerminalScreen::new(6, 30, 100);
        after.process_bytes(b"\x1bc\x1b]133;A\x1b\\$ ");
        assert_eq!(kinds(&after), [SemanticMarkKind::Prompt]);

        let pieces: [&[u8]; 5] = [
            b"\x1b]133;A\x1b\\$ ",
            b"\x1b]133;C\x1b\\",
            b"output\r\n",
            b"\x1bc",
            b"\x1b]133;A\x1b\\$ ",
        ];
        let mut split = TerminalScreen::new(6, 30, 100);
        for piece in pieces {
            split.process_bytes(piece);
        }
        let mut whole = TerminalScreen::new(6, 30, 100);
        whole.process_bytes(&pieces.concat());
        assert_eq!(kinds(&split), [SemanticMarkKind::Prompt]);
        assert_eq!(kinds(&whole), kinds(&split));
        assert_eq!(
            whole.semantic_marks()[0].absolute_line,
            split.semantic_marks()[0].absolute_line
        );
    }

    /// `clear` erases the screen and then the scrollback. The shell marks recorded before it must
    /// go with the lines they named, not slide onto whatever the screen shows next.
    #[test]
    fn clearing_scrollback_keeps_shell_marks_on_their_own_lines() {
        let mut screen = TerminalScreen::new(4, 30, 100);
        // One write per step, as a shell sends them: marks take the line a chunk ends on.
        let command = |screen: &mut TerminalScreen, output: &str| {
            for chunk in [
                "\x1b]133;A\x1b\\$ ".to_string(),
                "\x1b]133;B\x1b\\run\r\n".to_string(),
                "\x1b]133;C\x1b\\".to_string(),
                format!("{output}\r\n"),
                "\x1b]133;D;0\x1b\\".to_string(),
            ] {
                screen.process_bytes(chunk.as_bytes());
            }
        };
        for n in 0..6 {
            command(&mut screen, &format!("old output {n}"));
        }
        screen.process_bytes(b"\x1b[H\x1b[2J\x1b[3J");
        command(&mut screen, "fresh output");

        assert_eq!(
            screen
                .export_last_command_output()
                .as_deref()
                .map(str::trim_end),
            Some("fresh output")
        );
        // Only the command run after the clear is left to jump between; the cleared ones went
        // with their lines instead of landing on the new screen.
        let marks = screen.semantic_marks();
        assert_eq!(marks.len(), 4, "{marks:?}");
        assert!(
            marks.iter().all(|mark| mark.absolute_line <= 2),
            "{marks:?}"
        );
    }

    /// Attributes and underline color of a live-screen cell, read through the engine.
    fn live_cell(
        screen: &TerminalScreen,
        line: usize,
        col: usize,
    ) -> (CellAttrs, Option<CellColor>) {
        with_engine!(&screen.engine, engine => {
            let row = engine.row(engine.history_size() + line);
            let cell = &row.cells()[col];
            (CellRead::attrs(cell), CellRead::underline_color(cell))
        })
    }

    fn assert_replay_round_trips(source: &mut TerminalScreen) -> TerminalScreen {
        let replay = source.export_replay_bytes();
        let mut target =
            TerminalScreen::new(source.rows, source.cols, source.engine.scrollback_limit());
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
        assert_eq!(target_snapshot.hyperlinks, source_snapshot.hyperlinks);
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
            assert_eq!(
                target_snapshot.hyperlinks, source_snapshot.hyperlinks,
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
    fn capture_frame_keeps_terminal_color_semantics() {
        let mut screen = TerminalScreen::new(2, 6, 10);
        // A themed palette must not leak into the capture.
        screen.set_palette(TerminalColorPalette::new(
            UiColor::Rgb(1, 1, 1),
            UiColor::Rgb(2, 2, 2),
            [UiColor::Rgb(3, 3, 3); 16],
        ));
        screen.process_bytes(b"d\x1b[31mr\x1b[91mR\x1b[38;5;200mi\x1b[38;2;1;2;3mt\x1b[0;44mb");
        let frame = screen.capture_frame();
        let row = frame.row(0);

        assert_eq!((row[0].fg, row[0].bg), (UiColor::Reset, UiColor::Reset));
        assert_eq!(row[1].fg, UiColor::Red);
        assert_eq!(row[2].fg, UiColor::LightRed);
        assert_eq!(row[3].fg, UiColor::Indexed(200));
        assert_eq!(row[4].fg, UiColor::Rgb(1, 2, 3));
        assert_eq!((row[5].fg, row[5].bg), (UiColor::Reset, UiColor::Blue));
    }

    #[test]
    fn capture_frame_keeps_low_indexed_colors_indexed() {
        let mut screen = TerminalScreen::new(1, 4, 10);
        screen.process_bytes(b"\x1b[38;5;1mf\x1b[0;48;5;1mb\x1b[0;4;58;5;1mu\x1b[0m");
        let frame = screen.capture_frame();
        let row = frame.row(0);

        assert_eq!(row[0].fg, UiColor::Indexed(1));
        assert_eq!(row[1].bg, UiColor::Indexed(1));
        assert_eq!(row[2].underline_color, UiColor::Indexed(1));
        // A named underline color has no SGR 58 form, so this is what keeps it in the output.
        let underlined = frame.to_ansi_text();
        assert!(underlined.contains("\x1b[58;5;1mu"), "{underlined:?}");
    }

    #[test]
    fn capture_frame_reports_modifiers_and_underline_color() {
        let mut screen = TerminalScreen::new(1, 8, 10);
        screen.process_bytes(
            b"\x1b[1mb\x1b[0;2md\x1b[0;3mi\x1b[0;4:3;58:2::9:8:7mu\x1b[0;7mr\x1b[0;9ms",
        );
        let frame = screen.capture_frame();
        let row = frame.row(0);
        let modifiers = |cell: &CapturedCell| {
            let m = &cell.modifiers;
            [
                m.bold,
                m.dim,
                m.italic,
                m.underline.is_some(),
                m.reverse,
                m.strikethrough,
            ]
        };

        for (index, cell) in row[..6].iter().enumerate() {
            let mut expected = [false; 6];
            expected[index] = true;
            assert_eq!(
                modifiers(cell),
                expected,
                "cell {index} ({:?})",
                cell.symbol
            );
        }
        // A curly underline keeps its shape and its own color.
        assert_eq!(row[3].modifiers.underline, Some(UnderlineStyle::Curly));
        assert_eq!(row[3].underline_color, UiColor::Rgb(9, 8, 7));
        assert_eq!(row[0].underline_color, UiColor::Reset);
        assert_eq!(modifiers(&row[6]), [false; 6]);
    }

    #[test]
    fn capture_frame_keeps_each_underline_shape() {
        let mut screen = TerminalScreen::new(1, 6, 10);
        screen.process_bytes(b"\x1b[4ma\x1b[4:2mb\x1b[4:3mc\x1b[4:4md\x1b[4:5me\x1b[4:0mf");
        let frame = screen.capture_frame();
        let shapes: Vec<_> = frame.row(0).iter().map(|c| c.modifiers.underline).collect();

        assert_eq!(
            shapes,
            [
                Some(UnderlineStyle::Single),
                Some(UnderlineStyle::Double),
                Some(UnderlineStyle::Curly),
                Some(UnderlineStyle::Dotted),
                Some(UnderlineStyle::Dashed),
                None,
            ]
        );
        // Written back out, each shape keeps its `SGR 4:n` form.
        let ansi = frame.to_ansi_text();
        for expected in [
            "\x1b[4ma",
            "\x1b[4:2mb",
            "\x1b[4:3mc",
            "\x1b[4:4md",
            "\x1b[4:5me",
        ] {
            assert!(
                ansi.contains(expected),
                "{expected:?} missing from {ansi:?}"
            );
        }
    }

    #[test]
    fn capture_frame_keeps_styled_blank_cells() {
        let mut screen = TerminalScreen::new(2, 4, 10);
        // Erase-in-line paints the rest of the row with the current background.
        screen.process_bytes(b"\x1b[42mx\x1b[K\x1b[0m\r\n");
        let frame = screen.capture_frame();

        assert!(frame.row(0).iter().all(|cell| cell.bg == UiColor::Green));
        assert_eq!(frame.row(0)[3].symbol, " ");
        assert!(frame.row(1).iter().all(|cell| cell.bg == UiColor::Reset));
    }

    #[test]
    fn capture_frame_gives_wide_glyphs_their_display_width() {
        let mut screen = TerminalScreen::new(2, 5, 10);
        // The second glyph cannot fit in the last column, so it wraps and leaves a blank.
        screen.process_bytes("a中b中".as_bytes());
        let frame = screen.capture_frame();
        let symbols: Vec<&str> = frame.row(0).iter().map(|c| c.symbol.as_str()).collect();

        assert_eq!(symbols, ["a", "中", "", "b", " "]);
        assert_eq!(frame.row(1)[0].symbol, "中");
        assert_eq!(frame.row(1)[1].symbol, "");
        // Each row keeps all five columns, including the blank the wrapped glyph left behind.
        assert_eq!(frame.to_fixed_grid(), "a中b \n中   ");
    }

    #[test]
    fn capture_frame_hides_concealed_text_and_keeps_combining_marks() {
        let mut screen = TerminalScreen::new(1, 4, 10);
        screen.process_bytes("e\u{301}\x1b[8mpw\x1b[0m".as_bytes());
        let frame = screen.capture_frame();

        assert_eq!(frame.row(0)[0].symbol, "e\u{301}");
        assert_eq!(frame.row(0)[1].symbol, " ");
        assert_eq!(frame.row(0)[2].symbol, " ");
    }

    #[test]
    fn capture_frame_reports_the_cursor() {
        let mut screen = TerminalScreen::new(3, 6, 10);
        screen.process_bytes(b"ab\r\nc");
        let frame = screen.capture_frame();
        // The screen's default cursor is a blinking block with no color of its own.
        assert_eq!(frame.cursor, Some(CursorState::new(1, 1).blinking(true)));

        screen.process_bytes(b"\x1b[?25l");
        let cursor = screen.capture_frame().cursor.expect("cursor in viewport");
        assert!(!cursor.visible);
    }

    #[test]
    fn capture_frame_reports_the_programs_cursor_shape_blink_and_color() {
        let mut screen = TerminalScreen::new(2, 6, 10);
        let cursor = |screen: &TerminalScreen| screen.capture_frame().cursor.expect("cursor");

        // CSI 6 SP q: steady bar.
        screen.process_bytes(b"\x1b[6 q");
        let steady_bar = cursor(&screen);
        assert_eq!(steady_bar.shape, CursorShape::Bar);
        assert!(!steady_bar.blinking);

        // CSI 3 SP q: blinking underline.
        screen.process_bytes(b"\x1b[3 q");
        let underline = cursor(&screen);
        assert_eq!(underline.shape, CursorShape::Underline);
        assert!(underline.blinking);

        // CSI 2 SP q: steady block.
        screen.process_bytes(b"\x1b[2 q");
        assert_eq!(cursor(&screen).shape, CursorShape::Block);
        assert!(!cursor(&screen).blinking);
        assert_eq!(cursor(&screen).color, None);

        // OSC 12 sets the cursor color; OSC 112 resets it.
        screen.process_bytes(b"\x1b]12;#12ab34\x07");
        assert_eq!(cursor(&screen).color, Some(UiColor::Rgb(0x12, 0xab, 0x34)));
        screen.process_bytes(b"\x1b]112\x07");
        assert_eq!(cursor(&screen).color, None);
    }

    #[test]
    fn capture_frame_follows_the_scrolled_viewport() {
        let mut screen = TerminalScreen::new(2, 4, 10);
        screen.process_bytes(b"one\r\ntwo\r\nsix");
        screen.set_scrollback(1);
        let frame = screen.capture_frame();

        assert_eq!(frame.plain_text(), "one\ntwo");
        // The live cursor row is below the viewport while scrolled back.
        assert_eq!(frame.cursor, None);
    }

    #[test]
    fn unchanged_resize_preserves_cached_snapshot_and_pending_damage() {
        let mut screen = TerminalScreen::new(3, 8, 20);
        screen.process_bytes(b"before");
        let before = screen.render_snapshot();
        let _ = screen.take_damage();
        screen.resize(3, 8);
        let after = screen.render_snapshot();
        assert!(Arc::ptr_eq(&before.color_lines, &after.color_lines));
        assert_eq!(before.sequence, after.sequence);
        assert!(matches!(screen.take_damage(), TerminalDamage::None));

        screen.process_bytes(b"\x1b[2;1HX");
        screen.resize(3, 8);
        assert!(matches!(screen.take_damage(), TerminalDamage::Rows(rows)
            if rows.iter().any(|row| row.row == 1)));
        assert!(screen.render_snapshot().text.contains('X'));
        assert!(!before.text.contains('X'));

        screen.resize(4, 9);
        let frame = screen.capture_frame();
        assert_eq!((frame.height, frame.width), (4, 9));
    }

    #[test]
    fn unchanged_resize_preserves_alternate_screen_snapshot_and_pending_damage() {
        let mut screen = TerminalScreen::new(3, 8, 20);
        screen.process_bytes(b"primary\x1b[?1049h\x1b[Hbefore");
        let before = screen.render_snapshot();
        assert!(screen.alt_screen);
        assert!(before.text.contains("before"));
        let _ = screen.take_damage();

        screen.resize(3, 8);
        let after = screen.render_snapshot();
        assert!(Arc::ptr_eq(&before.color_lines, &after.color_lines));
        assert_eq!(before.sequence, after.sequence);
        assert!(matches!(screen.take_damage(), TerminalDamage::None));

        screen.process_bytes(b"\x1b[2;1HX");
        screen.resize(3, 8);
        assert!(matches!(screen.take_damage(), TerminalDamage::Rows(rows)
            if rows.iter().any(|row| row.row == 1)));
        let updated = screen.render_snapshot();
        assert!(screen.alt_screen);
        assert!(updated.text.contains('X'));
        assert!(!before.text.contains('X'));

        screen.process_bytes(b"\x1b[?1049l");
        let primary = screen.render_snapshot();
        assert!(!screen.alt_screen);
        assert!(primary.text.contains("primary"));
        assert!(!primary.text.contains('X'));
    }

    #[test]
    fn resize_clamps_before_checking_for_unchanged_dimensions() {
        let mut screen = TerminalScreen::new(1, 1, 20);
        let before = screen.render_snapshot();
        screen.resize(0, 0);
        let after = screen.render_snapshot();
        assert!(Arc::ptr_eq(&before.color_lines, &after.color_lines));
        assert_eq!(
            (screen.capture_frame().width, screen.capture_frame().height),
            (1, 1)
        );
    }

    #[test]
    fn capture_frame_tracks_resize() {
        let mut screen = TerminalScreen::new(2, 4, 10);
        screen.process_bytes(b"hi");
        screen.resize(3, 7);
        let frame = screen.capture_frame();

        assert_eq!((frame.width, frame.height), (7, 3));
        assert_eq!(frame.cells.len(), 21);
        assert_eq!((frame.viewport.w, frame.viewport.h), (7, 3));
        assert_eq!(frame.plain_text(), "hi\n\n");
    }

    #[test]
    fn render_snapshot_reports_explicit_hyperlinks_per_visible_row() {
        let mut screen = TerminalScreen::new(3, 6, 10);
        screen.process_bytes(
            b"\x1b]8;id=docs;https://example.com/docs\x1b\\abcdefgh\x1b]8;;\x1b\\ plain",
        );
        let snapshot = screen.render_snapshot();

        assert_eq!(
            snapshot.hyperlinks.as_ref(),
            &[
                TerminalHyperlink::new("https://example.com/docs", 0, 0..6),
                TerminalHyperlink::new("https://example.com/docs", 1, 0..2),
            ]
        );
        assert!(snapshot.hyperlinks[0].contains(0, 5));
        assert!(!snapshot.hyperlinks[0].contains(1, 0));
    }

    #[test]
    fn snapshot_keeps_one_run_across_default_space_tails() {
        let mut screen = TerminalScreen::new(1, 20, 0);
        screen.process_bytes(b"hello");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0].len(), 1);
        assert_eq!(
            snapshot.color_lines[0][0].content.as_ref(),
            "hello               "
        );
    }

    #[test]
    fn snapshot_merges_named_and_indexed_palette_red() {
        let mut screen = TerminalScreen::new(1, 4, 0);
        screen.process_bytes(b"\x1b[31mA\x1b[38;5;1mB");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "AB");
        assert_eq!(
            snapshot.color_lines[0][0].style.fg,
            Some(UiColor::Red.into())
        );
    }

    #[test]
    fn snapshot_merges_underline_variants() {
        let mut screen = TerminalScreen::new(1, 4, 0);
        screen.process_bytes(b"\x1b[4mA\x1b[4:2mB");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "AB");
        assert_eq!(snapshot.color_lines[0][0].style.underline, Some(true));
    }

    #[test]
    fn snapshot_splits_when_bold_changes() {
        let mut screen = TerminalScreen::new(1, 4, 0);
        screen.process_bytes(b"\x1b[1mA\x1b[22mB");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "A");
        assert_eq!(snapshot.color_lines[0][0].style.bold, Some(true));
        assert_eq!(snapshot.color_lines[0][1].content.as_ref(), "B  ");
        assert_eq!(snapshot.color_lines[0][1].style.bold, Some(false));
    }

    #[test]
    fn snapshot_keeps_hidden_cells_in_the_same_run() {
        let mut screen = TerminalScreen::new(1, 6, 0);
        screen.process_bytes(b"ab\x1b[8mcd\x1b[28mef");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0].len(), 1);
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "ab  ef");
    }

    #[test]
    fn snapshot_hyperlink_does_not_split_style_run() {
        let mut screen = TerminalScreen::new(1, 8, 0);
        screen.process_bytes(b"ab\x1b]8;;https://e.com\x1b\\cd\x1b]8;;\x1b\\ef");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0].len(), 1);
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "abcdef  ");
        assert_eq!(snapshot.hyperlinks.len(), 1);
        assert_eq!(snapshot.hyperlinks[0].start_col, 2);
        assert_eq!(snapshot.hyperlinks[0].end_col, 4);
    }

    #[test]
    fn snapshot_wide_char_spacer_stays_in_one_run() {
        let mut screen = TerminalScreen::new(1, 4, 0);
        screen.process_bytes("あx".as_bytes());
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0].len(), 1);
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "あx ");
    }

    #[test]
    fn snapshot_alternating_styles_create_one_span_per_run() {
        let mut screen = TerminalScreen::new(1, 4, 0);
        screen.process_bytes(b"\x1b[31mA\x1b[32mB\x1b[31mC\x1b[32mD");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0].len(), 4);
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "A");
        assert_eq!(snapshot.color_lines[0][1].content.as_ref(), "B");
        assert_eq!(snapshot.color_lines[0][2].content.as_ref(), "C");
        assert_eq!(snapshot.color_lines[0][3].content.as_ref(), "D");
    }

    #[test]
    fn snapshot_inverse_and_dim_split_runs() {
        let mut screen = TerminalScreen::new(1, 4, 0);
        screen.process_bytes(b"\x1b[7mA\x1b[27mB\x1b[2mC");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0][0].style.reverse, Some(true));
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "A");
        assert_eq!(snapshot.color_lines[0][1].style.reverse, Some(false));
        assert_eq!(snapshot.color_lines[0][1].content.as_ref(), "B");
        assert_eq!(snapshot.color_lines[0][2].style.dim, Some(true));
        assert_eq!(snapshot.color_lines[0][2].content.as_ref(), "C");
    }

    #[test]
    fn alias_continuation_moves_the_run_key_to_the_new_color() {
        let palette = TerminalColorPalette::default();
        let named = CellColor::Named(NamedColor::Red);
        let indexed = CellColor::Indexed(1);
        let bg = CellColor::BACKGROUND;
        let bits = style_run_bits(CellAttrs::empty());
        let (mapped_fg, mapped_bg) =
            style_run_progress(None, named, bg, bits, &palette).expect("first cell starts a run");
        let mut key = StyleRunKey {
            fg: named,
            bg,
            mapped_fg,
            mapped_bg,
            bits,
        };

        assert!(style_run_progress(Some(&mut key), indexed, bg, bits, &palette).is_none());
        assert_eq!(key.fg, indexed);
        assert_eq!(key.bg, bg);
        assert_eq!(key.mapped_fg, mapped_fg);
        assert_eq!(key.mapped_bg, mapped_bg);
    }

    #[test]
    fn snapshot_keeps_one_run_after_an_alias_color_change() {
        let mut screen = TerminalScreen::new(1, 6, 0);
        screen.process_bytes(b"\x1b[31mA\x1b[38;5;1mBCDEF");
        let snapshot = screen.render_snapshot();
        assert_eq!(snapshot.color_lines[0].len(), 1);
        assert_eq!(snapshot.color_lines[0][0].content.as_ref(), "ABCDEF");
    }

    #[test]
    fn style_run_bits_match_style_for_every_flag_combination() {
        let style_flags = [
            CellAttrs::BOLD,
            CellAttrs::DIM,
            CellAttrs::ITALIC,
            CellAttrs::UNDERLINE,
            CellAttrs::INVERSE,
            CellAttrs::STRIKEOUT,
        ];
        let combos: Vec<CellAttrs> = (0..1u8 << style_flags.len())
            .map(|mask| {
                style_flags
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mask & (1 << index) != 0)
                    .fold(CellAttrs::empty(), |flags, (_, flag)| flags | *flag)
            })
            .collect();

        for &flags in &combos {
            let style = style_from_mapped(None, None, style_run_bits(flags));
            assert_eq!(style.bold, Some(flags.contains(CellAttrs::BOLD)));
            assert_eq!(style.dim, Some(flags.contains(CellAttrs::DIM)));
            assert_eq!(style.italic, Some(flags.contains(CellAttrs::ITALIC)));
            assert_eq!(
                style.underline,
                Some(flags.intersects(CellAttrs::ALL_UNDERLINES))
            );
            assert_eq!(style.reverse, Some(flags.contains(CellAttrs::INVERSE)));
            assert_eq!(
                style.strikethrough,
                Some(flags.contains(CellAttrs::STRIKEOUT))
            );
        }
        for &a in &combos {
            for &b in &combos {
                assert_eq!(
                    style_run_bits(a) == style_run_bits(b),
                    style_from_mapped(None, None, style_run_bits(a))
                        == style_from_mapped(None, None, style_run_bits(b)),
                );
            }
        }
    }

    #[test]
    fn style_run_bits_ignore_non_style_flags() {
        assert_eq!(
            style_run_bits(CellAttrs::HIDDEN | CellAttrs::WIDE_CHAR),
            style_run_bits(CellAttrs::empty())
        );
        assert_eq!(
            style_run_bits(CellAttrs::UNDERLINE),
            style_run_bits(CellAttrs::DOUBLE_UNDERLINE)
        );
    }

    #[test]
    fn nonzero_prompt_anchor_reflows_with_unterminated_output() {
        for input in ["hello", "abcdefghijklmnopqrstuvwxyz1234567890"] {
            let mut screen = TerminalScreen::new(8, 120, 20);
            let output = "x".repeat(40);
            let prompt = format!("prompt > \x1b]133;B\x1b\\{input}");
            let expected = format!("{output}prompt > {input}");
            screen.process_bytes(output.as_bytes());
            screen.process_bytes(b"\x1b]133;A\x1b\\");
            screen.process_bytes(prompt.as_bytes());

            for _ in 0..3 {
                screen.resize(8, 32);
                assert_eq!(
                    screen.active_prompt_mark,
                    Some(ActivePromptMark {
                        absolute_line: 1,
                        column: 8
                    })
                );
                // A column-addressed redraw isolates reflow from a shell's CR overwriting
                // unterminated output. It does not emit a fresh OSC 133;A.
                screen.process_bytes(b"\x1b[1;9H\x1b[K");
                screen.process_bytes(prompt.as_bytes());
                let narrow = screen.render_snapshot();
                let expected_lines: Vec<_> = expected
                    .as_bytes()
                    .chunks(32)
                    .map(|chunk| {
                        std::str::from_utf8(chunk)
                            .expect("ASCII test input")
                            .to_owned()
                    })
                    .collect();
                assert_eq!(screen.text_lines(0, expected_lines.len()), expected_lines);
                assert_eq!(
                    (narrow.cursor_row, narrow.cursor_col),
                    (
                        ((8 + 9 + input.len()) / 32) as u16,
                        ((8 + 9 + input.len()) % 32) as u16
                    )
                );

                screen.resize(8, 120);
                assert_eq!(
                    screen.active_prompt_mark,
                    Some(ActivePromptMark {
                        absolute_line: 0,
                        column: 40
                    })
                );
                screen.process_bytes(b"\x1b[1;41H\x1b[K");
                screen.process_bytes(prompt.as_bytes());
                let wide = screen.render_snapshot();
                assert_eq!(
                    wide.text.lines().next().map(str::trim_end),
                    Some(expected.as_str())
                );
                assert_eq!(
                    (wide.cursor_row, wide.cursor_col),
                    (0, expected.len() as u16)
                );
                assert!(wide.text.lines().skip(1).all(|line| line.trim().is_empty()));
            }
            screen.process_bytes(b"x");
            let snapshot = screen.render_snapshot();
            assert_eq!(
                snapshot.text.lines().next().map(str::trim_end),
                Some(format!("{expected}x").as_str())
            );
            assert_eq!(
                (snapshot.cursor_row, snapshot.cursor_col),
                (0, expected.len() as u16 + 1)
            );
        }
    }

    #[test]
    fn tall_active_prompt_survives_repeated_height_shrinks() {
        for short_rows in [2, 3] {
            let mut screen = TerminalScreen::new(8, 120, 40);
            let input = "abcdefghij".repeat(80);
            let expected = format!("prompt > {input}");
            let prompt = b"prompt > \x1b]133;B\x1b\\";
            screen.process_bytes(b"\x1b]133;A\x1b\\");
            screen.process_bytes(prompt);
            screen.process_bytes(input.as_bytes());

            for _ in 0..3 {
                for rows in [short_rows, 8] {
                    screen.resize(rows, 120);
                    assert!(
                        screen.active_prompt_mark.is_some(),
                        "a retained history anchor is valid"
                    );
                    if rows == short_rows {
                        let mark = screen.active_prompt_mark.expect("retained anchor");
                        assert!(mark.absolute_line < screen.engine.history_size());
                    }
                    // Readline clears its seven-row input and redraws without OSC 133;A.
                    screen.process_bytes(b"\r\x1b[K\r");
                    for _ in 0..6 {
                        screen.process_bytes(b"\x1b[A\x1b[K\r");
                    }
                    screen.process_bytes(prompt);
                    screen.process_bytes(input.as_bytes());
                }
            }
            screen.process_bytes(b"x");
            let actual: String = screen.text_lines(0, screen.total_text_lines()).concat();
            assert_eq!(actual, format!("{expected}x"));
            assert_eq!(screen.render_snapshot().cursor_col, 90);
        }
    }

    #[test]
    fn shrinking_drops_a_prompt_anchor_only_after_history_eviction() {
        let mut screen = TerminalScreen::new(8, 120, 2);
        screen.process_bytes(b"\x1b]133;A\x1b\\prompt > \x1b]133;B\x1b\\");
        screen.process_bytes("abcdefghij".repeat(80).as_bytes());
        screen.resize(2, 120);
        assert_eq!(screen.engine.history_size(), 2);
        assert!(screen.evicted_lines > 0);
        assert!(
            screen.active_prompt_mark.is_none(),
            "the prompt start was actually evicted"
        );
    }

    #[test]
    fn bash_prompt_cursor_survives_shrink_then_grow() {
        for narrow_rows in [8, 3] {
            let mut screen = TerminalScreen::new(8, 120, 20);
            let prompt = b"project branch > \x1b]133;B\x1b\\";
            let input = b"abcdefghijklmnopqrstuvwxyz1234567890";
            screen.process_bytes(b"previous command\r\nprevious output\r\n");
            screen.process_bytes(b"\x1b]133;A\x1b\\");
            screen.process_bytes(prompt);
            screen.process_bytes(input);

            for _ in 0..3 {
                // Bash/readline output after SIGWINCH at each width. PROMPT_COMMAND's
                // OSC 133;A is absent from redraws, while PS1's OSC 133;B is repeated.
                screen.resize(narrow_rows, 32);
                screen.process_bytes(b"\r\x1b[K\r");
                screen.process_bytes(prompt);
                screen.process_bytes(input);
                let narrow = screen.render_snapshot();
                assert_eq!(
                    (narrow.cursor_row, narrow.cursor_col),
                    (3.min(narrow_rows - 1), 21)
                );
                assert_eq!(screen.text_lines(0, 1), ["previous command"]);

                screen.resize(8, 120);
                screen.process_bytes(b"\r\x1b[K\r\x1b[A\x1b[K\r");
                screen.process_bytes(prompt);
                screen.process_bytes(input);
                let wide = screen.render_snapshot();
                assert_eq!((wide.cursor_row, wide.cursor_col), (2, 53));
            }
            screen.process_bytes(b"x");

            let snapshot = screen.render_snapshot();
            assert_eq!(
                snapshot.text.lines().next().map(str::trim_end),
                Some("previous command")
            );
            assert_eq!(
                snapshot.text.lines().nth(1).map(str::trim_end),
                Some("previous output")
            );
            assert_eq!(
                snapshot.text.lines().nth(2).map(str::trim_end),
                Some("project branch > abcdefghijklmnopqrstuvwxyz1234567890x")
            );
            assert_eq!((snapshot.cursor_row, snapshot.cursor_col), (2, 54));
        }
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
    fn a_utf8_prompt_glyph_does_not_hide_the_command_start() {
        // `❯` is `E2 9D AF`: its `0x9D` continuation byte is not a C1 OSC introducer, so the
        // `OSC 133;C` in the next chunk must still retire the prompt mark.
        let mut screen = TerminalScreen::new(4, 24, 20);
        screen.process_bytes(b"\x1b]133;A\x1b\\");
        screen.process_bytes("dir ❯ command\r\n".as_bytes());
        screen.process_bytes(b"\x1b]133;C;vendor_exe=command\x1b\\");
        screen.process_bytes(b"command output");

        screen.resize(4, 18);

        let text = screen.render_snapshot().text;
        assert!(text.contains("dir ❯ command"), "{text}");
        assert!(text.contains("command output"), "{text}");
    }

    /// Prompt boundaries are found with the engine's own OSC framing.
    #[test]
    fn prompt_boundaries_follow_vt_osc_framing() {
        let mut screen = TerminalScreen::new(4, 20, 10);
        // Raw C1 bytes are UTF-8 continuation bytes, not OSC/ST.
        screen.process_bytes(b"\x9d133;A\x9c");
        assert_eq!(screen.active_prompt_mark, None);
        // An ESC ends the open OSC, so a following `ESC ]` starts a fresh one.
        screen.process_bytes(b"\r\nxy\x1b]0;title\x1b]133;A\x07$ ");
        assert_eq!(
            screen.active_prompt_mark,
            Some(ActivePromptMark {
                absolute_line: 1,
                column: 2,
            })
        );
        screen.process_bytes(b"ls\x1b]133;C\x1b\\");
        assert_eq!(screen.active_prompt_mark, None);
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
            ansi_reported: HostTerminalColors::ALL_ANSI_REPORTED,
            cursor: None,
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
            ansi_reported: HostTerminalColors::ALL_ANSI_REPORTED,
            cursor: None,
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
    fn answers_fragmented_xtversion_queries_with_its_configured_identity() {
        let mut screen = TerminalScreen::new(2, 8, 10);
        assert_eq!(
            screen.terminal_identity(),
            concat!("tui-lipan ", env!("CARGO_PKG_VERSION"))
        );
        screen.set_terminal_identity(" \x1bRozi 1.2\u{e9}\x07 ");
        assert_eq!(screen.terminal_identity(), "Rozi 1.2");

        screen.process_bytes(b"\x1b[>");
        assert!(screen.drain_responses().is_empty());
        screen.process_bytes(b"0q\x1b[>q\x1b[>1q\x1b[6 q");

        assert_eq!(
            screen.drain_responses(),
            vec![
                b"\x1bP>|Rozi 1.2\x1b\\".to_vec(),
                b"\x1bP>|Rozi 1.2\x1b\\".to_vec(),
            ]
        );
        assert!(screen.render_snapshot().text.trim().is_empty());
    }

    #[test]
    fn reset_preserves_identity_but_discards_a_partial_xtversion_query() {
        let mut screen = TerminalScreen::new(2, 8, 10);
        screen.set_terminal_identity("mux 2.0");
        screen.process_bytes(b"\x1b[>");

        screen.reset();
        screen.process_bytes(b"0q");

        assert_eq!(screen.terminal_identity(), "mux 2.0");
        assert!(screen.drain_responses().is_empty());
    }

    #[test]
    fn xtversion_reporting_is_enabled_by_default_and_can_be_disabled() {
        let mut screen = TerminalScreen::new(2, 8, 10);
        assert!(screen.xtversion_enabled());

        screen.set_xtversion_enabled(false);
        screen.process_bytes(b"\x1b[>0q");
        assert!(!screen.xtversion_enabled());
        assert!(screen.drain_responses().is_empty());

        screen.set_xtversion_enabled(true);
        screen.process_bytes(b"\x1b[>0q");
        assert_eq!(
            screen.drain_responses(),
            vec![format!("\x1bP>|tui-lipan {}\x1b\\", env!("CARGO_PKG_VERSION")).into_bytes()]
        );
    }

    #[test]
    fn xtversion_response_preserves_order_with_primary_parser_responses() {
        let mut screen = TerminalScreen::new(2, 8, 10);
        screen.set_terminal_identity("mux 2.0");
        let xtversion = b"\x1bP>|mux 2.0\x1b\\";

        screen.process_bytes(b"\x1b[>0q\x1b]10;?\x1b\\");
        let responses = screen.drain_responses();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0], xtversion);
        assert!(responses[1].starts_with(b"\x1b]10;"), "{responses:?}");

        screen.process_bytes(b"\x1b]10;?\x1b\\\x1b[>0q");
        let responses = screen.drain_responses();
        assert_eq!(responses.len(), 2);
        assert!(responses[0].starts_with(b"\x1b]10;"), "{responses:?}");
        assert_eq!(responses[1], xtversion);
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

    /// Kitty's `icat` waits for the DA1 reply after its graphics probes and only accepts one with
    /// more than `?6c` inside, so the emulator's VT102 answer left it hanging until its timeout.
    #[test]
    fn answers_primary_device_attributes_as_a_vt220_in_stream_order() {
        let mut screen = TerminalScreen::new(4, 20, 10);
        screen.process_bytes(b"\x1b]10;?\x1b\\\x1b[c\x1b[0c");
        let responses = screen.drain_responses();
        assert_eq!(responses.len(), 3, "{responses:?}");
        assert!(responses[0].starts_with(b"\x1b]10;"), "{responses:?}");
        assert_eq!(responses[1], b"\x1b[?62;22c");
        assert_eq!(responses[2], b"\x1b[?62;22c");

        screen.process_bytes(b"\x1b[>c");
        let answered = String::from_utf8(screen.drain_responses().concat()).unwrap();
        // Secondary device attributes identify the emulator.
        let identity = "\x1b[>0;";
        assert!(answered.starts_with(identity), "{answered:?}");
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
    fn replay_round_trips_hyperlink_cells_and_active_template() {
        let mut source = TerminalScreen::new(2, 24, 10);
        source.process_bytes(b"\x1b]8;;https://example.com/docs\x1b\\docs");

        let mut target = assert_replay_round_trips(&mut source);
        assert_eq!(
            target.render_snapshot().hyperlinks.as_ref(),
            &[TerminalHyperlink::new("https://example.com/docs", 0, 0..4)]
        );

        // The source left OSC 8 open. Replay restores the cursor template too, so later bytes sent
        // to source and target remain linked without another opening sequence.
        source.process_bytes(b" next");
        target.process_bytes(b" next");
        assert_eq!(
            target.render_snapshot().hyperlinks,
            source.render_snapshot().hyperlinks
        );
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
            (0, CellAttrs::DOUBLE_UNDERLINE),
            (1, CellAttrs::UNDERCURL),
            (2, CellAttrs::DOTTED_UNDERLINE),
            (3, CellAttrs::DASHED_UNDERLINE),
            (4, CellAttrs::HIDDEN),
        ] {
            let (source_attrs, source_underline) = live_cell(&source, 0, col);
            let (target_attrs, target_underline) = live_cell(&target, 0, col);
            assert!(source_attrs.contains(flags), "source col {col}");
            assert!(target_attrs.contains(flags), "target col {col}");
            assert_eq!(
                target_attrs.intersection(flags),
                source_attrs.intersection(flags)
            );
            assert_eq!(target_underline, source_underline);
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
    fn replay_writer_matches_collected_export() {
        #[derive(Default)]
        struct OneByteWriter(Vec<u8>);

        impl std::io::Write for OneByteWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                let accepted = bytes.len().min(1);
                self.0.extend_from_slice(&bytes[..accepted]);
                Ok(accepted)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut screen = TerminalScreen::new(3, 12, 10);
        screen.process_bytes("primary \x1b[31m漢e\u{301}\r\nline".as_bytes());
        screen.process_bytes(b"\x1b[?1049halt \x1b]8;;https://example.com\x1b\\link");

        let expected = screen.export_replay_bytes();
        let mut written = OneByteWriter::default();
        screen.write_replay_bytes(&mut written).unwrap();

        assert_eq!(written.0, expected);
    }

    #[test]
    fn replay_writer_error_still_repairs_alt_screen_source() {
        struct FailAfter {
            remaining: usize,
        }

        impl std::io::Write for FailAfter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.remaining == 0 {
                    return Err(std::io::Error::other("intentional writer failure"));
                }
                let accepted = bytes.len().min(self.remaining);
                self.remaining -= accepted;
                Ok(accepted)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut screen = TerminalScreen::new(3, 12, 20);
        screen.process_bytes(b"primary one\r\nprimary two\r\nprimary three");
        screen.process_bytes(b"\x1b[?1049halt \x1b[32mscreen\x1b[2;4H");
        let _ = screen.take_damage();
        // Alacritty keeps a selection in the emulator that the swap must preserve too.
        let alacritty = matches!(screen.engine, Engine::Alacritty(_));
        let mut selection = alacritty_terminal::selection::Selection::new(
            alacritty_terminal::selection::SelectionType::Simple,
            alacritty_terminal::index::Point::new(Line(0), Column(0)),
            alacritty_terminal::index::Side::Left,
        );
        selection.update(
            alacritty_terminal::index::Point::new(Line(0), Column(2)),
            alacritty_terminal::index::Side::Right,
        );
        if alacritty {
            screen.engine.alacritty_mut().term.selection = Some(selection.clone());
        }
        let expected = screen.export_replay_bytes();
        let selection_kept = |screen: &TerminalScreen| {
            !alacritty || screen.engine.alacritty().term.selection == Some(selection.clone())
        };
        assert!(selection_kept(&screen));
        std::assert_matches!(screen.take_damage(), TerminalDamage::None);
        let before = screen.render_snapshot();

        for remaining in [0, 1, expected.len() / 2, expected.len() - 1] {
            let error = screen
                .write_replay_bytes(&mut FailAfter { remaining })
                .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::Other);

            let after = screen.render_snapshot();
            assert_eq!(after.text, before.text);
            assert_eq!(after.color_lines, before.color_lines);
            assert_eq!(after.cursor_row, before.cursor_row);
            assert_eq!(after.cursor_col, before.cursor_col);
            assert!(selection_kept(&screen));
            std::assert_matches!(screen.take_damage(), TerminalDamage::None);
            assert_eq!(screen.export_replay_bytes(), expected);
            assert!(selection_kept(&screen));
            std::assert_matches!(screen.take_damage(), TerminalDamage::None);
        }
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
    fn late_host_defaults_preserve_only_cursor_properties_the_child_set() {
        use crate::style::TerminalCursorStyle;
        for (request, shape, blinking) in [
            (b"\x1b[?12h".as_slice(), CaretShape::Bar, true),
            (b"\x1b[?12l", CaretShape::Bar, false),
            (b"\x1b[?12$p", CaretShape::Bar, false),
            (b"\x1b]50;CursorShape=2\x07", CaretShape::Underline, false),
            (b"\x1b[3 q", CaretShape::Underline, true),
            (b"\x1b[0 q", CaretShape::Bar, false),
        ] {
            let mut screen = TerminalScreen::new(4, 10, 20);
            screen.process_bytes(request);
            screen.apply_default_cursor_style(TerminalCursorStyle {
                shape: CaretShape::Bar,
                blinking: false,
            });
            let snapshot = screen.render_snapshot();
            assert_eq!(snapshot.cursor_shape, shape);
            assert_eq!(snapshot.cursor_blinking, blinking);
        }
    }

    #[test]
    fn cursor_blink_reports_follow_the_default_in_force_when_the_child_asks() {
        use crate::style::TerminalCursorStyle;
        let steady_bar = TerminalCursorStyle {
            shape: CaretShape::Bar,
            blinking: false,
        };
        // Before late binding, a query answers from the standalone blinking-block default.
        let mut screen = TerminalScreen::new(4, 10, 20);
        screen.process_bytes(b"\x1b[?12$p");
        assert_eq!(screen.drain_responses(), vec![b"\x1b[?12;1$y".to_vec()]);
        screen.apply_default_cursor_style(steady_bar);
        screen.process_bytes(b"\x1b[?12$p");
        assert_eq!(screen.drain_responses(), vec![b"\x1b[?12;2$y".to_vec()]);
        assert!(!screen.render_snapshot().cursor_blinking);

        // Seeding before the first byte answers the very first query with the host preference.
        let mut seeded = TerminalScreen::new(4, 10, 20);
        seeded.set_default_cursor_style(steady_bar);
        seeded.process_bytes(b"\x1b[?12$p");
        assert_eq!(seeded.drain_responses(), vec![b"\x1b[?12;2$y".to_vec()]);
    }

    #[test]
    fn configured_cursor_defaults_survive_resets_and_preserve_child_requests() {
        use crate::style::TerminalCursorStyle;
        let mut screen = TerminalScreen::new(4, 10, 20);
        let steady_bar = TerminalCursorStyle {
            shape: CaretShape::Bar,
            blinking: false,
        };
        screen.set_default_cursor_style(steady_bar);
        let initial = screen.render_snapshot();
        assert_eq!(initial.cursor_shape, CaretShape::Bar);
        assert!(!initial.cursor_blinking);
        assert!(!screen.capture_frame().cursor.unwrap().blinking);
        screen.process_bytes(b"\x1b[3 q");
        screen.set_default_cursor_style(TerminalCursorStyle {
            shape: CaretShape::Block,
            blinking: false,
        });
        let explicit = screen.render_snapshot();
        assert_eq!(explicit.cursor_shape, CaretShape::Underline);
        assert!(explicit.cursor_blinking);
        screen.set_default_cursor_style(steady_bar);
        screen.process_bytes(b"\x1b[0 q");
        assert_eq!(screen.render_snapshot().cursor_shape, CaretShape::Bar);
        assert!(!screen.render_snapshot().cursor_blinking);
        screen.process_bytes(b"\x1b[?12h");
        assert!(screen.render_snapshot().cursor_blinking);
        screen.process_bytes(b"\x1bc");
        assert!(!screen.render_snapshot().cursor_blinking);
        screen.reset();
        assert_eq!(screen.render_snapshot().cursor_shape, CaretShape::Bar);
        assert!(!screen.render_snapshot().cursor_blinking);
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
    fn logical_text_lines_join_soft_wraps_and_partition_physical_ranges() {
        let mut screen = TerminalScreen::new(4, 5, 10);
        screen.process_bytes(b"abcdefgh\r\nnext");
        let total = screen.total_text_lines();
        let mut lines = Vec::new();

        let flow = screen.try_for_each_logical_text_line(0, total, |line| {
            lines.push((
                line.text().to_string(),
                line.absolute_lines(),
                line.start_position(5),
                line.end_position(5),
            ));
            ControlFlow::Continue(())
        });

        assert_eq!(flow, ControlFlow::Continue(()));
        let wrapped = lines
            .iter()
            .find(|(text, _, _, _)| text == "abcdefgh")
            .expect("soft-wrapped logical line");
        assert_eq!(wrapped.1, 0..2);
        assert_eq!(
            wrapped.2,
            Some(TerminalTextPosition {
                absolute_line: 1,
                column: 0,
            })
        );
        assert_eq!(
            wrapped.3,
            Some(TerminalTextPosition {
                absolute_line: 0,
                column: 5,
            })
        );

        let mut first_range = Vec::new();
        let _ = screen.try_for_each_logical_text_line(0, 1, |line| {
            first_range.push(line.text().to_string());
            ControlFlow::Continue(())
        });
        let mut continuation_range = Vec::new();
        let _ = screen.try_for_each_logical_text_line(1, 2, |line| {
            continuation_range.push(line.text().to_string());
            ControlFlow::Continue(())
        });
        assert_eq!(first_range, ["abcdefgh"]);
        assert!(continuation_range.is_empty());
    }

    #[test]
    fn logical_text_positions_reject_invalid_utf8_boundaries() {
        let mut screen = TerminalScreen::new(2, 5, 0);
        screen.process_bytes("漢字abc".as_bytes());
        let mut checked = false;

        let _ = screen.try_for_each_logical_text_line(0, 1, |line| {
            if line.text().starts_with('漢') {
                assert_eq!(line.start_position(1), None);
                assert_eq!(line.end_position(line.text().len() + 1), None);
                assert_eq!(
                    line.start_position("漢".len()),
                    Some(TerminalTextPosition {
                        absolute_line: 0,
                        column: 2,
                    })
                );
                checked = true;
            }
            ControlFlow::Continue(())
        });
        assert!(checked);
    }

    #[test]
    fn logical_text_positions_follow_terminal_cells_for_unicode_sequences() {
        for (text, x_column, inside_cell_boundary) in [
            ("1️⃣x", 1, Some('1'.len_utf8())),
            ("❤️x", 1, Some('❤'.len_utf8())),
            ("e\u{301}x", 1, Some('e'.len_utf8())),
            ("漢x", 2, None),
        ] {
            let mut screen = TerminalScreen::new(2, 8, 0);
            screen.process_bytes(text.as_bytes());
            let mut checked = false;

            let _ = screen.try_for_each_logical_text_line(0, 1, |line| {
                if line.text() != text {
                    return ControlFlow::Continue(());
                }
                let x_offset = line.text().find('x').unwrap();
                let x_position = TerminalTextPosition {
                    absolute_line: 0,
                    column: x_column,
                };
                assert_eq!(line.start_position(x_offset), Some(x_position), "{text:?}");
                assert_eq!(line.end_position(x_offset), Some(x_position), "{text:?}");
                assert_eq!(
                    line.end_position(x_offset + 'x'.len_utf8()),
                    Some(TerminalTextPosition {
                        absolute_line: 0,
                        column: x_column + 1,
                    }),
                    "{text:?}"
                );
                if let Some(byte_index) = inside_cell_boundary {
                    assert_eq!(
                        line.start_position(byte_index),
                        Some(TerminalTextPosition {
                            absolute_line: 0,
                            column: 0,
                        }),
                        "{text:?}"
                    );
                    assert_eq!(
                        line.end_position(byte_index),
                        Some(TerminalTextPosition {
                            absolute_line: 0,
                            column: x_column,
                        }),
                        "{text:?}"
                    );
                }
                checked = true;
                ControlFlow::Continue(())
            });
            assert!(checked, "missing logical line for {text:?}");
        }
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
        let marks = b"\x1b]133;A\x1b\\\x1b]133;C\x1b\\".as_slice();
        for chunks in [
            vec![b"\x1b[?1049h".to_vec(), marks.to_vec()],
            vec![[b"\x1b[?1049h".as_slice(), marks].concat()],
        ] {
            let mut screen = TerminalScreen::new(5, 20, 50);
            for chunk in chunks {
                screen.process_bytes(&chunk);
            }
            screen.process_bytes(b"altstuff\r\n");
            screen.process_bytes(b"\x1b[?1049l");
            screen.process_bytes(b"back-on-main\r\n");

            assert!(
                screen.semantic_marks().is_empty(),
                "alt-screen OSC 133 must not be replayed against main-screen lines"
            );
            assert_eq!(screen.last_command_output_range(), None);
        }
    }

    #[test]
    fn marks_in_one_chunk_land_on_their_own_lines() {
        let mut screen = TerminalScreen::new(10, 20, 50);
        screen.process_bytes(
            b"\x1b]133;A\x1b\\$ ls\r\n\x1b]133;C\x1b\\one\r\ntwo\r\n\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ ",
        );

        assert_eq!(
            screen.export_last_command_output().as_deref(),
            Some("one\ntwo")
        );
    }

    #[test]
    fn marks_inside_a_synchronized_update_land_on_their_own_lines() {
        let frame =
            b"\x1b[?2026h\x1b]133;A\x1b\\$ ls\r\n\x1b]133;C\x1b\\one\r\n\x1b[?2026l".as_slice();
        let end = b"\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ ".as_slice();
        let split = frame.len() - b"one\r\n\x1b[?2026l".len();
        for chunks in [
            vec![[frame, end].concat()],
            vec![
                frame[..split].to_vec(),
                frame[split..].to_vec(),
                end.to_vec(),
            ],
        ] {
            let mut screen = TerminalScreen::new(10, 20, 50);
            for chunk in chunks {
                screen.process_bytes(&chunk);
            }

            assert_eq!(screen.export_last_command_output().as_deref(), Some("one"));
        }
    }

    #[test]
    fn reset_inside_a_synchronized_update_keeps_the_marks_after_it() {
        let mut screen = TerminalScreen::new(10, 20, 50);
        screen
            .process_bytes(b"\x1b]133;A\x1b\\$ old\r\n\x1b]133;C\x1b\\stale\r\n\x1b]133;D;0\x1b\\");
        screen.process_bytes(
            b"\x1b[?2026h\x1bc\x1b]133;A\x1b\\$ ls\r\n\x1b]133;C\x1b\\two\r\n\x1b[?2026l\x1b]133;D;0\x1b\\",
        );

        let kinds: Vec<_> = screen
            .semantic_marks()
            .iter()
            .map(|mark| mark.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                SemanticMarkKind::Prompt,
                SemanticMarkKind::OutputStart,
                SemanticMarkKind::OutputEnd
            ],
            "the reset drops the marks before it and only those"
        );
        assert_eq!(screen.export_last_command_output().as_deref(), Some("two"));
    }

    #[test]
    fn full_screen_program_resolves_to_the_command_before_it() {
        let mut screen = TerminalScreen::new(4, 20, 50);
        screen.process_bytes(b"\x1b]133;A\x1b\\$ ls\r\n");
        screen.process_bytes(b"\x1b]133;C\x1b\\");
        screen.process_bytes(b"files\r\n");
        screen.process_bytes(b"\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ vim\r\n");
        // Shells emit `C` just before running the command, so it shares a chunk with the
        // program's switch to the alternate screen.
        screen.process_bytes(b"\x1b]133;C\x1b\\\x1b[?1049h\x1b[2;3Hvim-ui");

        let starts = screen
            .semantic_marks()
            .iter()
            .filter(|mark| mark.kind == SemanticMarkKind::OutputStart)
            .count();
        assert_eq!(starts, 2, "the full-screen program's start mark is kept");
        assert_eq!(
            screen.export_last_command_output().as_deref(),
            Some("files")
        );
        assert!(screen.is_alternate_screen());
        assert_eq!(
            screen.text_lines(0, screen.total_text_lines()),
            vec!["", "  vim-ui", "", ""],
            "reading the primary screen must leave the alternate screen as it was"
        );
        assert_eq!(screen.engine.cursor(), GridPoint { line: 1, column: 8 });

        screen.process_bytes(b"\x1b[?1049l\x1b]133;D;0\x1b\\\x1b]133;A\x1b\\$ ");
        assert_eq!(
            screen.export_last_command_output().as_deref(),
            Some(""),
            "a full-screen program leaves no output on the primary screen"
        );
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
        fn a_placement_inside_a_synchronized_update_uses_that_updates_cursor() {
            let mut screen = screen(30, 70, 10);
            let payload = BASE64.encode([0x80, 0x80, 0x80]);
            let stream = format!(
                "\x1b[30;70H\
                 \x1b[?2026h\
                 \x1b[2;6H\
                 \x1b_Ga=t,f=24,s=1,v=1,t=d,i=7;{payload}\x1b\\\
                 \x1b_Ga=p,i=7,p=1,c=14,r=6,C=1,z=-1499999999\x1b\\\
                 \x1b[?2026l"
            );

            screen.process_bytes(stream.as_bytes());

            let snapshot = screen.render_snapshot();
            assert_eq!(snapshot.images.len(), 1);
            let placement = &snapshot.images[0];
            assert_eq!((placement.row, placement.col), (1, 5));
            assert_eq!((placement.rows, placement.cols), (6, 14));
            assert_eq!(placement.z, -1_499_999_999);
        }

        #[test]
        fn replay_restores_direct_images_in_scrollback() {
            let mut source = screen(5, 20, 50);
            source.process_bytes(b"zero\r\none\r\ntwo\r\n");
            source.process_bytes(&place(
                40,
                60,
                "i=7,p=3,C=1,c=2,r=2,x=10,y=20,w=20,h=40,z=-9",
            ));
            source.process_bytes(b"\r\n".repeat(6).as_slice());

            let replay = source.export_replay_bytes();
            let mut restored = screen(5, 20, 50);
            restored.process_bytes(&replay);
            assert!(restored.has_images());

            source.set_scrollback(4);
            restored.set_scrollback(4);
            let expected = source.render_snapshot();
            let actual = restored.render_snapshot();
            assert_eq!(actual.images.len(), 1);
            assert_eq!(actual.images[0].image_id, expected.images[0].image_id);
            assert_eq!(
                (actual.images[0].row, actual.images[0].col),
                (expected.images[0].row, expected.images[0].col)
            );
            assert_eq!((actual.images[0].rows, actual.images[0].cols), (2, 2));
            assert_eq!(actual.images[0].z, -9);
            assert_eq!(
                actual.images[0].source_crop,
                Some(TerminalImageCrop {
                    x: 10,
                    y: 20,
                    width: 20,
                    height: 40,
                })
            );
            assert_eq!(
                actual.images[0].image.pixels().unwrap().to_rgba8(),
                expected.images[0].image.pixels().unwrap().to_rgba8()
            );
        }

        #[test]
        fn replay_places_an_image_on_a_wrapped_continuation_line() {
            let mut source = screen(4, 5, 20);
            source.process_bytes(b"abcdef");
            source.process_bytes(&place(10, 20, "i=7,C=1"));

            let replay = source.export_replay_bytes();
            let mut restored = screen(4, 5, 20);
            restored.process_bytes(&replay);
            let expected = source.render_snapshot();
            let actual = restored.render_snapshot();
            assert_eq!(actual.text, expected.text);
            assert_eq!(actual.wrapped_rows, expected.wrapped_rows);
            assert_eq!(actual.images.len(), 1);
            assert_eq!(
                (actual.images[0].row, actual.images[0].col),
                (expected.images[0].row, expected.images[0].col)
            );
        }

        #[test]
        fn replay_restores_virtual_images_and_image_number_aliases() {
            let cell = TerminalCellSize::new(10, 20);
            let mut source = screen(10, 40, 50);
            source.process_bytes(&placeholders(7, 6, 3, cell));
            source.process_bytes(b"\x1b_Ga=T,U=1,c=6,r=3,f=24,s=1,v=1,t=d,i=7,I=42,q=2;YGBg\x1b\\");

            let replay = source.export_replay_bytes();
            let mut restored = screen(10, 40, 50);
            restored.process_bytes(&replay);
            assert_eq!(restored.render_snapshot().images.len(), 1);

            source.process_bytes(b"\x1b[5;9H\x1b_Ga=p,I=42,c=1,r=1,C=1,q=2;\x1b\\");
            restored.process_bytes(b"\x1b[5;9H\x1b_Ga=p,I=42,c=1,r=1,C=1,q=2;\x1b\\");
            let expected = source.render_snapshot();
            let actual = restored.render_snapshot();
            assert_eq!(actual.images.len(), expected.images.len());
            assert_eq!(
                (actual.images[1].row, actual.images[1].col),
                (expected.images[1].row, expected.images[1].col)
            );
        }

        #[test]
        fn replay_keeps_automatic_image_ids_in_sync_with_future_output() {
            let mut source = screen(6, 20, 10);
            source.process_bytes(&place(10, 20, "C=1"));

            let replay = source.export_replay_bytes();
            let mut restored = screen(6, 20, 10);
            restored.process_bytes(&replay);

            source.process_bytes(b"\x1b[2;1H");
            restored.process_bytes(b"\x1b[2;1H");
            source.process_bytes(&place(10, 20, "C=1"));
            restored.process_bytes(&place(10, 20, "C=1"));
            let expected_ids: Vec<_> = source
                .render_snapshot()
                .images
                .iter()
                .map(|placement| placement.image_id)
                .collect();
            let actual_ids: Vec<_> = restored
                .render_snapshot()
                .images
                .iter()
                .map(|placement| placement.image_id)
                .collect();
            assert_eq!(actual_ids, expected_ids);
        }

        #[test]
        fn replay_restores_images_on_both_alternate_screen_grids() {
            let mut source = screen(6, 20, 10);
            source.process_bytes(&place(10, 20, "i=1,C=1"));
            source.process_bytes(b"\x1b[?1049h\x1b[3;4H");
            source.process_bytes(&place(20, 40, "i=2,C=1,z=5"));

            let replay = source.export_replay_bytes();
            let mut restored = screen(6, 20, 10);
            restored.process_bytes(&replay);
            let alt = restored.render_snapshot();
            assert_eq!(alt.images.len(), 1);
            assert_eq!(alt.images[0].image_id, 2);
            assert_eq!((alt.images[0].row, alt.images[0].col), (2, 3));

            source.process_bytes(b"\x1b[?1049l");
            restored.process_bytes(b"\x1b[?1049l");
            let expected = source.render_snapshot();
            let primary = restored.render_snapshot();
            assert_eq!(primary.images.len(), 1);
            assert_eq!(primary.images[0].image_id, 1);
            assert_eq!(
                (primary.images[0].row, primary.images[0].col),
                (expected.images[0].row, expected.images[0].col)
            );
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

        /// Feed `setup` to two fresh screens, then `parts` to one in a single write and to the
        /// other one part per write, and return both.
        fn fed_whole_and_in_parts(
            rows: u16,
            scrollback: usize,
            setup: &[u8],
            parts: &[&[u8]],
        ) -> (TerminalScreen, TerminalScreen) {
            let mut whole = screen(rows, 20, scrollback);
            let mut split = screen(rows, 20, scrollback);
            whole.process_bytes(setup);
            split.process_bytes(setup);
            whole.process_bytes(&parts.concat());
            for part in parts {
                split.process_bytes(part);
            }
            (whole, split)
        }

        fn placements(screen: &mut TerminalScreen) -> Vec<(i32, i32)> {
            screen
                .render_snapshot()
                .images
                .iter()
                .map(|placement| (placement.row, placement.col))
                .collect()
        }

        #[test]
        fn clearing_scrollback_in_the_same_write_as_an_image_keeps_the_image() {
            // `clear; icat`, as a multiplexer relays it: the history `clear` erases and the image
            // drawn after it can arrive in one write. The erased lines were gone before the image
            // was placed, so they must not move it.
            let history = b"line\r\n".repeat(12);
            let image = place(20, 40, "i=1");
            let (mut whole, mut split) =
                fed_whole_and_in_parts(4, 50, &history, &[b"\x1b[H\x1b[2J\x1b[3J", &image]);

            assert_eq!(placements(&mut split), vec![(0, 0)]);
            assert_eq!(placements(&mut whole), placements(&mut split));
        }

        #[test]
        fn clearing_scrollback_inside_a_synchronized_update_keeps_the_image_drawn_after_it() {
            // The same frame with the clear buffered by a synchronized update, which commits only
            // when the image command arrives.
            let history = b"line\r\n".repeat(12);
            let image = place(20, 40, "i=1");
            let (mut whole, mut split) = fed_whole_and_in_parts(
                4,
                50,
                &history,
                &[b"\x1b[?2026h\x1b[H\x1b[2J\x1b[3J", &image, b"\x1b[?2026l"],
            );

            assert_eq!(placements(&mut split), vec![(0, 0)]);
            assert_eq!(placements(&mut whole), placements(&mut split));
        }

        #[test]
        fn lines_evicted_after_an_image_in_the_same_write_still_move_it() {
            // The opposite order: a four-row image is placed first, then output pushes its top
            // rows out of a one-line history. Those evictions happened to the image and must
            // shift it.
            let image = place(20, 80, "i=1");
            let lines = b"x\r\n".repeat(3);
            let (mut whole, mut split) = fed_whole_and_in_parts(4, 1, b"", &[&image, &lines]);

            assert_eq!(placements(&mut split).len(), 1);
            assert_eq!(placements(&mut whole), placements(&mut split));
        }

        #[test]
        fn an_alternate_screen_left_in_the_same_write_takes_its_images_with_it() {
            let image = place(20, 40, "i=1,C=1");
            let (mut whole, mut split) =
                fed_whole_and_in_parts(6, 10, b"", &[b"\x1b[?1049h", &image, b"\x1b[?1049l"]);
            whole.process_bytes(b"\x1b[?1049h");
            split.process_bytes(b"\x1b[?1049h");

            assert!(placements(&mut split).is_empty());
            assert!(
                placements(&mut whole).is_empty(),
                "a fresh alternate screen shows the previous one's image"
            );
        }

        #[test]
        fn an_alternate_screen_left_and_reopened_in_one_write_starts_without_images() {
            // No graphics command between the exit and the re-entry, so nothing but the exit
            // itself can say the old alternate screen and its image are gone.
            let setup = [b"\x1b[?1049h".as_slice(), &place(20, 40, "i=1,C=1")].concat();
            let (mut whole, mut split) =
                fed_whole_and_in_parts(6, 10, &setup, &[b"\x1b[?1049l", b"\x1b[?1049h"]);

            assert!(placements(&mut split).is_empty());
            assert!(
                placements(&mut whole).is_empty(),
                "a fresh alternate screen shows the previous one's image"
            );
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

        /// Where the snapshot shows each image, as `(row, col)`.
        fn image_cells(screen: &mut TerminalScreen) -> Vec<(i32, i32)> {
            screen
                .render_snapshot()
                .images
                .iter()
                .map(|image| (image.row, image.col))
                .collect()
        }

        /// The viewport row whose text starts with `prefix`.
        fn row_of(screen: &mut TerminalScreen, prefix: &str) -> i32 {
            screen
                .snapshot()
                .lines()
                .position(|line| line.starts_with(prefix))
                .expect("the line is on screen") as i32
        }

        /// A long line, then `ab` with an image placed right after it, at column 2.
        fn image_after_a_long_line(rows: u16, cols: u16) -> TerminalScreen {
            let mut screen = screen(rows, cols, 50);
            screen.process_bytes("x".repeat(35).as_bytes());
            screen.process_bytes(b"\r\nab");
            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            screen
        }

        /// A rewrap moves the text above an image, and the image has to move with it rather than
        /// vanish: a pane resized while showing pictures otherwise loses every one of them for
        /// good, since nothing redraws them.
        #[test]
        fn a_widening_rewrap_moves_placements_with_their_text() {
            // The long line takes two rows at this width, one once the pane widens.
            let mut screen = image_after_a_long_line(8, 20);
            assert_eq!(image_cells(&mut screen), vec![(2, 2)]);

            screen.resize(8, 40);
            assert_eq!(row_of(&mut screen, "ab"), 1);
            assert_eq!(image_cells(&mut screen), vec![(1, 2)]);
        }

        #[test]
        fn a_narrowing_rewrap_moves_placements_with_their_text() {
            let mut screen = image_after_a_long_line(8, 40);
            assert_eq!(image_cells(&mut screen), vec![(1, 2)]);

            for cols in [20, 40, 12] {
                screen.resize(8, cols);
                let row = row_of(&mut screen, "ab");
                assert_eq!(
                    image_cells(&mut screen),
                    vec![(row, 2)],
                    "at {cols} columns"
                );
            }
        }

        /// The marks that carry anchors through a rewrap are bookkeeping, never content.
        #[test]
        fn a_rewrap_leaves_no_anchor_marks_in_the_text() {
            let mut screen = image_after_a_long_line(8, 20);
            screen.resize(8, 40);
            screen.resize(8, 20);

            // Alacritty carries these marks with their cells during reflow.
            if !matches!(screen.engine, Engine::Alacritty(_)) {
                return;
            }
            let grid = screen.engine.alacritty().term.grid();
            for line in grid.topmost_line().0..=grid.bottommost_line().0 {
                for column in 0..grid.columns() {
                    assert!(
                        grid[Line(line)][Column(column)]
                            .zerowidth()
                            .is_none_or(<[char]>::is_empty),
                        "a mark survived at {line},{column}"
                    );
                }
            }
            assert!(!screen.snapshot().contains('\u{FDD0}'));
        }

        /// A height-only resize rewraps nothing, so it must not mark anything either: a mark left
        /// behind was found again by the next width change and miscounted, which panicked the
        /// session server.
        #[test]
        fn a_height_change_leaves_no_anchor_marks() {
            let mut screen = image_after_a_long_line(8, 20);
            screen.resize(10, 20);
            screen.resize(10, 40);
            screen.resize(12, 40);
            screen.resize(12, 20);

            // Alacritty carries these marks with their cells during reflow.
            if !matches!(screen.engine, Engine::Alacritty(_)) {
                return;
            }
            let grid = screen.engine.alacritty().term.grid();
            for line in grid.topmost_line().0..=grid.bottommost_line().0 {
                for column in 0..grid.columns() {
                    assert!(
                        grid[Line(line)][Column(column)]
                            .zerowidth()
                            .is_none_or(<[char]>::is_empty),
                        "a mark survived at {line},{column}"
                    );
                }
            }
            let row = row_of(&mut screen, "ab");
            assert_eq!(image_cells(&mut screen), vec![(row, 2)]);
        }

        /// Text pushed out of scrollback by the rewrap takes its image with it.
        #[test]
        fn a_rewrap_that_evicts_the_anchor_drops_its_placement() {
            let mut screen = screen(2, 40, 0);
            screen.process_bytes(&place(20, 40, "i=1,C=1"));
            screen.process_bytes("x".repeat(35).as_bytes());
            screen.process_bytes(b"\r\nab");
            assert_eq!(image_cells(&mut screen), vec![(0, 0)]);

            // The long line needs two rows at this width, and with no scrollback the row holding
            // the anchor has nowhere to go.
            screen.resize(2, 20);
            assert!(image_cells(&mut screen).is_empty());
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

    fn assert_incremental_snapshot_matches_full(screen: &mut TerminalScreen) {
        let incremental = screen.render_snapshot();
        screen.snapshot_damage.mark_full();
        screen.dirty = true;
        let full = screen.render_snapshot();
        assert_eq!(incremental.text, full.text);
        assert_eq!(incremental.color_lines, full.color_lines);
        assert_eq!(incremental.wrapped_rows, full.wrapped_rows);
        assert_eq!(incremental.hyperlinks, full.hyperlinks);
        assert_eq!(incremental.cursor_row, full.cursor_row);
        assert_eq!(incremental.cursor_col, full.cursor_col);
        assert_eq!(incremental.cursor_visible, full.cursor_visible);
        assert_eq!(incremental.key_modes, full.key_modes);
        assert_eq!(incremental.mouse_mode, full.mouse_mode);
    }

    #[test]
    fn incremental_snapshots_match_full_grid_across_terminal_operations() {
        let mut screen = TerminalScreen::new(8, 20, 30);
        let updates: &[&[u8]] = &[
            b"first\r\nsecond\r\nthird",
            b"\x1b[2;2H\x1b[31mX\x1b[0m",
            "\x1b[3;1H你e\u{301}🙂".as_bytes(),
            b"\x1b[2;1H\x1b[2K",
            b"\x1b]8;;https://example.test\x07link\x1b]8;;\x07",
            b"\x1b[2;1Hgone",
            b"\x1b]2;title only\x07",
            b"\x1b[?25l",
            b"\x1b[8;1Hbottom\r\nscroll",
            b"\x1b[2;7r\x1b[7;1H\n",
            b"\x1b[r\x1b[?1049h",
            b"alternate",
            b"\x1b[1;2H/",
            b"\x1b[?1049l",
            b"\x1b[2J",
            b"\x1b[H01234567890123456789012345",
            b"\x1b[1;20HX",
            b"\x1b[2;1H\x1b[@",
            b"\x1b[P",
        ];
        screen.render_snapshot();
        for update in updates {
            // Reads and paints may consume damage in either order.
            screen.process_bytes(update);
            let _ = screen.take_damage();
            assert_incremental_snapshot_matches_full(&mut screen);
        }
        // Batches must accumulate all changed rows, not just the final write's damage.
        for update in updates {
            screen.process_bytes(update);
        }
        assert_incremental_snapshot_matches_full(&mut screen);
        for (rows, cols) in [(4, 10), (12, 30), (8, 20)] {
            screen.resize(rows, cols);
            assert_incremental_snapshot_matches_full(&mut screen);
        }
        for offset in [1, 10, 0] {
            screen.set_scrollback(offset);
            assert_incremental_snapshot_matches_full(&mut screen);
        }
        screen.set_palette(TerminalColorPalette {
            foreground: Some(crate::style::Color::Red),
            ..Default::default()
        });
        assert_incremental_snapshot_matches_full(&mut screen);
        screen.reset();
        assert_incremental_snapshot_matches_full(&mut screen);
    }

    #[test]
    fn snapshot_reuses_clean_rows_without_consuming_paint_damage() {
        let mut screen = TerminalScreen::new(4, 20, 10);
        screen.process_bytes(b"unchanged\r\nstatus");
        let before = screen.render_snapshot();
        let _ = screen.take_damage();
        screen.process_bytes(b"\x1b[2;1H/");
        let after = screen.render_snapshot();
        assert!(Arc::ptr_eq(
            &before.color_lines[0][0].content,
            &after.color_lines[0][0].content
        ));
        assert_ne!(before.color_lines[1], after.color_lines[1]);
        assert!(matches!(screen.take_damage(), TerminalDamage::Rows(_)));
        assert_eq!(before.text.lines().nth(1).unwrap().trim(), "status");
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
