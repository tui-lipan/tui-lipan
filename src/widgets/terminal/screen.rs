use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, GridCell, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags as CellFlags};
use alacritty_terminal::term::{self, Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor as VteProcessor;
use alacritty_terminal::vte::ansi::{
    Color as TermColor, CursorShape as TermCursorShape, CursorStyle as TermCursorStyle, NamedColor,
    Rgb as TermRgb,
};

use super::events::{
    KittyKeyboardFlags, MouseEncoding, MouseMode, MouseModeState, TerminalKeyModes,
};
use crate::style::{CaretShape, Color as UiColor, HostTerminalColors, Span, Style};

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
/// When the terminal parser encounters escape sequences that require a response
/// (e.g., device attributes queries, cursor position reports), alacritty_terminal
/// generates `Event::PtyWrite` events. This listener captures those responses
/// so they can be written back to the PTY.
#[derive(Clone, Default)]
struct ResponseCapture {
    responses: Rc<RefCell<Vec<Vec<u8>>>>,
    /// Latest window title set by the program via OSC 0/2; `None` once reset.
    title: Rc<RefCell<Option<String>>>,
    /// The active palette, shared with [`TerminalScreen`], used to answer
    /// `OSC 4/10/11 ; ?` color queries so guest programs don't block waiting
    /// for a reply (see [`Self::resolve_query_color`]).
    palette: Rc<RefCell<TerminalColorPalette>>,
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
            TermEvent::Title(title) => *self.title.borrow_mut() = Some(title),
            TermEvent::ResetTitle => *self.title.borrow_mut() = None,
            // Answer color queries from the active palette. Without this the
            // guest blocks until its own timeout (e.g. tui-lipan's host-color
            // refresh), since alacritty delegates the reply to the listener.
            TermEvent::ColorRequest(index, formatter) => {
                let response = formatter(self.resolve_query_color(index));
                self.responses.borrow_mut().push(response.into_bytes());
            }
            // Ignore other events (Clipboard, Bell, etc.) for now
            _ => {}
        }
    }
}

/// Alacritty terminal screen parser for PTY output.
pub struct TerminalScreen {
    processor: VteProcessor,
    term: Term<ResponseCapture>,
    listener: ResponseCapture,
    /// Logical viewport rows (matches the PTY size).
    rows: u16,
    /// Logical viewport cols (matches the PTY size).
    cols: u16,
    scrollback_len: usize,
    mouse_mode: MouseModeState,
    scrollback_offset: usize,
    cache: TerminalRenderSnapshot,
    palette: TerminalColorPalette,
    dirty: bool,
    sequence: u64,
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
    /// Current mouse mode state.
    pub mouse_mode: MouseModeState,
    /// Input-affecting DEC private modes the child has enabled (DECCKM, bracketed paste).
    pub key_modes: TerminalKeyModes,
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
            mouse_mode: MouseModeState::default(),
            key_modes: TerminalKeyModes::default(),
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
            mouse_mode,
            key_modes,
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
        let config = TermConfig {
            scrolling_history: scrollback,
            default_cursor_style: DEFAULT_CURSOR_STYLE,
            // Track Kitty keyboard protocol pushes so `key_modes()` can report what the child
            // negotiated; without this alacritty silently drops every `CSI > <flags> u`.
            kitty_keyboard: true,
            ..TermConfig::default()
        };
        let listener = ResponseCapture::default();
        let term = Term::new(config, &dimensions, listener.clone());
        Self {
            processor: VteProcessor::new(),
            term,
            listener,
            rows,
            cols,
            scrollback_len: scrollback,
            mouse_mode: MouseModeState::default(),
            scrollback_offset: 0,
            cache: TerminalRenderSnapshot::default(),
            palette: TerminalColorPalette::default(),
            dirty: true,
            sequence: 0,
        }
    }

    /// Feed terminal bytes.
    ///
    pub fn process_bytes(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
        self.scrollback_offset = self.term.grid().display_offset();
        self.mouse_mode = mouse_mode_from_term(*self.term.mode());
        self.dirty = true;
    }

    /// Drain and return any PTY responses that need to be written back.
    ///
    /// Call this after `process_bytes()` to get responses like device attribute
    /// queries, cursor position reports, etc. These should be written back to
    /// the PTY stdin.
    pub fn drain_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.listener.responses.borrow_mut())
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
    /// keyboard stack depth, hyperlinks, and the current display offset. The
    /// receiver lands on the live view.
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
        self.rows = rows.max(1);
        self.cols = cols.max(1);
        let dimensions = TermDimensions {
            rows: self.rows as usize,
            cols: self.cols as usize,
        };
        self.term.resize(dimensions);
        self.scrollback_offset = self.term.grid().display_offset();
        self.mouse_mode = mouse_mode_from_term(*self.term.mode());
        self.dirty = true;
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
            self.mouse_mode = mouse_mode_from_term(mode);

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
                mouse_mode: self.mouse_mode,
                key_modes: key_modes_from_term(mode),
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
        self.processor = VteProcessor::new();
        self.mouse_mode = MouseModeState::default();
        self.scrollback_offset = 0;
        self.cache = TerminalRenderSnapshot::default();
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
        push_dec_mode(bytes, 2004, mode.contains(TermMode::BRACKETED_PASTE));
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
        let content = cell_text(cell);
        if run_style == Some(style) {
            run_text.push_str(&content);
        } else {
            if let Some(prev_row) = current_row {
                flush_run(prev_row, &mut run_style, &mut run_text, &mut lines);
            }
            run_style = Some(style);
            run_text.push_str(&content);
        }
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

fn cell_text(cell: &TermCell) -> String {
    let mut text = String::new();
    let ch = if cell.flags.contains(CellFlags::HIDDEN) {
        ' '
    } else {
        cell.c
    };
    text.push(ch);
    if let Some(zerowidth) = cell.zerowidth() {
        for ch in zerowidth {
            text.push(*ch);
        }
    }
    text
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

fn mouse_mode_from_term(mode: TermMode) -> MouseModeState {
    let encoding = if mode.contains(TermMode::SGR_MOUSE) {
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
        screen.process_bytes(b"\x1b[?25l\x1b[?1003h\x1b[?1006h\x1b[?1004h\x1b[?2004h");

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
}
