use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags as CellFlags};
use alacritty_terminal::term::{self, Config as TermConfig, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor as VteProcessor;
use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor, Rgb as TermRgb};

use super::events::{MouseEncoding, MouseMode, MouseModeState};
use crate::style::{Color as UiColor, HostTerminalColors, Span, Style};

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
    /// Stable sequence key for cache invalidation.
    pub sequence: u64,
    /// Current scrollback offset (0 = live view, >0 = scrolled into history).
    pub scrollback_offset: usize,
    /// Total number of scrollback rows available.
    pub total_scrollback_rows: usize,
    /// Current mouse mode state.
    pub mouse_mode: MouseModeState,
}

impl Default for TerminalRenderSnapshot {
    fn default() -> Self {
        Self {
            text: Arc::from(""),
            color_lines: Arc::new([vec![Span::new("")]]),
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            sequence: 0,
            scrollback_offset: 0,
            total_scrollback_rows: 0,
            mouse_mode: MouseModeState::default(),
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
        sequence: u64,
        scrollback_offset: usize,
        total_scrollback_rows: usize,
        mouse_mode: MouseModeState,
    ) -> Self {
        Self {
            text: text.into(),
            color_lines: Arc::from(color_lines.into_boxed_slice()),
            cursor_row,
            cursor_col,
            cursor_visible,
            sequence,
            scrollback_offset,
            total_scrollback_rows,
            mouse_mode,
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
                sequence: self.sequence,
                scrollback_offset: self.scrollback_offset,
                total_scrollback_rows: self.term.history_size(),
                mouse_mode: self.mouse_mode,
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

    /// The window title the program has set via OSC 0/2 (e.g. the shell's
    /// `$PWD` or a running program's name). Returns `None` if no title has been
    /// set or it was reset. Updated as bytes are processed.
    pub fn title(&self) -> Option<String> {
        self.listener.title.borrow().clone()
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
}
