//! The emulator boundary behind [`TerminalScreen`](super::TerminalScreen).
//!
//! `TerminalScreen` owns everything that is tui-lipan logic: damage and snapshot accumulators,
//! mutation generations, the semantic mark history, graphics and media, the program-status store,
//! selection projection and presentation. Everything that parses VT bytes into a grid lives behind
//! [`TerminalEngine`], so a second emulator can sit beside the default one.
//!
//! The interface is shaped after libghostty-vt rather than after `alacritty_terminal`:
//!
//! - **In-band notifications.** An engine reports what happened while it parsed through an
//!   [`EngineObserver`], in stream order: PTY replies, the OSC sequences tui-lipan interprets
//!   itself, history evictions and hard resets. When the observer asks for an anchor, the engine
//!   hands it back with a [`CursorAnchor`] captured at the exact byte that asked, which is what a
//!   native callback sees. An engine that cannot report those sequences in-band reports them from
//!   [`TerminalEngine::scan_ahead`] instead, and positions the anchors once the screen has fed the
//!   bytes before them ([`TerminalEngine::anchor`]). Alacritty does that with a second VT parser.
//! - **Engine-provided line identity.** Evictions are counted by the engine
//!   ([`GridStep::evicted`]), and points that must follow text through a rewrap are tracked by it
//!   ([`ResizeRequest::track`]). Alacritty counts through the scrollback ledger and tracks with
//!   marker characters; Ghostty has tracked grid references for both.
//! - **Both screens readable.** [`TerminalEngine::with_primary_screen`] reads the primary screen
//!   while the alternate one is active. Alacritty swaps grids and repairs; Ghostty reads it.
//!
//! Grid reads are generic ([`TerminalEngine::row`] returns a concrete row type per engine) and
//! dispatch happens once per call through [`Engine`], never per cell.

use super::super::damage::TerminalDamage;
use super::{TerminalCellSize, TerminalClipboardEvent, TerminalColorPalette};

pub(super) mod alacritty;
#[cfg(feature = "terminal-ghostty")]
pub(super) mod ghostty;
mod semantic;

pub(super) use alacritty::AlacrittyEngine;
#[cfg(test)]
pub(in crate::widgets::terminal) use semantic::SemanticTestDriver;

/// The emulator behind a [`TerminalScreen`](super::TerminalScreen).
///
/// Unstable: selects the experimental libghostty-vt engine (feature `terminal-ghostty`) through
/// [`TerminalScreen::with_engine`](super::TerminalScreen::with_engine).
/// [`TerminalScreen::new`](super::TerminalScreen::new) always uses
/// [`TerminalEngineKind::Alacritty`].
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TerminalEngineKind {
    /// `alacritty_terminal`, the default.
    #[default]
    Alacritty,
    /// libghostty-vt 1.0.1, built through `tui-lipan-ghostty-vt-sys`. Experimental.
    #[cfg(feature = "terminal-ghostty")]
    Ghostty,
}

/// DEC private mode for SGR-pixel mouse reporting.
pub(super) const SGR_PIXELS_MOUSE: u16 = 1016;

/// Primary device attributes (DA1) reply: a VT220 (`62`) with ANSI color (`22`), as Ghostty sends.
pub(super) const PRIMARY_DEVICE_ATTRIBUTES: &[u8] = b"\x1b[?62;22c";

/// What an engine is built with.
#[derive(Clone, Copy, Debug)]
pub(super) struct EngineConfig {
    pub(super) rows: u16,
    pub(super) cols: u16,
    /// Scrollback lines exposed to callers.
    pub(super) scrollback: usize,
    pub(super) default_cursor_style: EngineCursorStyle,
}

/// Named terminal colors as a cell stores them.
///
/// The `Dim*`, `BrightForeground`, `DimForeground` and `Cursor` slots exist because Alacritty can
/// store them; tui-lipan maps them like Alacritty does (see `map_named_color`). Discriminants
/// follow Alacritty's so converting a cell color compiles down to a copy on the hot path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum NamedColor {
    Black = 0,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Foreground = 256,
    Background,
    Cursor,
    DimBlack,
    DimRed,
    DimGreen,
    DimYellow,
    DimBlue,
    DimMagenta,
    DimCyan,
    DimWhite,
    BrightForeground,
    DimForeground,
}

/// A cell color as the program set it, before any palette.
///
/// `Named(Red)` (`SGR 31`) and `Indexed(1)` (`SGR 38;5;1`) are kept apart: they render alike but
/// capture differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CellColor {
    Named(NamedColor),
    Rgb(u8, u8, u8),
    Indexed(u8),
}

impl CellColor {
    pub(super) const FOREGROUND: Self = Self::Named(NamedColor::Foreground);
    pub(super) const BACKGROUND: Self = Self::Named(NamedColor::Background);
}

/// Per-cell attributes. Bit values are private to this module; engines convert.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct CellAttrs(u16);

impl CellAttrs {
    pub(super) const INVERSE: Self = Self(1);
    pub(super) const BOLD: Self = Self(1 << 1);
    pub(super) const ITALIC: Self = Self(1 << 2);
    pub(super) const UNDERLINE: Self = Self(1 << 3);
    /// Reserved: Alacritty keeps the row's soft-wrap flag on its last cell. Use
    /// [`RowRead::wrapped`] instead.
    const WRAPLINE: Self = Self(1 << 4);
    /// First cell of a double-width glyph.
    pub(super) const WIDE_CHAR: Self = Self(1 << 5);
    /// Second cell of a double-width glyph.
    pub(super) const WIDE_CHAR_SPACER: Self = Self(1 << 6);
    pub(super) const DIM: Self = Self(1 << 7);
    pub(super) const HIDDEN: Self = Self(1 << 8);
    pub(super) const STRIKEOUT: Self = Self(1 << 9);
    /// Last column of a row whose double-width glyph wrapped to the next row.
    pub(super) const LEADING_WIDE_CHAR_SPACER: Self = Self(1 << 10);
    pub(super) const DOUBLE_UNDERLINE: Self = Self(1 << 11);
    pub(super) const UNDERCURL: Self = Self(1 << 12);
    pub(super) const DOTTED_UNDERLINE: Self = Self(1 << 13);
    pub(super) const DASHED_UNDERLINE: Self = Self(1 << 14);
    pub(super) const ALL_UNDERLINES: Self = Self(
        Self::UNDERLINE.0
            | Self::DOUBLE_UNDERLINE.0
            | Self::UNDERCURL.0
            | Self::DOTTED_UNDERLINE.0
            | Self::DASHED_UNDERLINE.0,
    );
    pub(super) const SPACERS: Self =
        Self(Self::WIDE_CHAR_SPACER.0 | Self::LEADING_WIDE_CHAR_SPACER.0);

    pub(super) const fn empty() -> Self {
        Self(0)
    }

    pub(super) const fn bits(self) -> u16 {
        self.0
    }

    /// Attributes from raw bits laid out like the constants above. Unknown bits are dropped.
    pub(super) const fn from_bits_truncate(bits: u16) -> Self {
        Self(bits & !Self::WRAPLINE.0 & 0x7fff)
    }

    pub(super) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(super) const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub(super) const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl std::ops::BitOr for CellAttrs {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// A hyperlink attached to a cell (`OSC 8`).
pub(super) trait LinkRead: Clone + PartialEq {
    fn uri(&self) -> &str;
}

/// One grid cell, read without copying it.
pub(super) trait CellRead {
    type Link: LinkRead;

    /// The base character. A blank cell holds `' '`.
    fn ch(&self) -> char;
    /// Combining characters after [`Self::ch`].
    fn zerowidth(&self) -> &[char];
    fn fg(&self) -> CellColor;
    fn bg(&self) -> CellColor;
    /// `SGR 58` underline color, when set.
    fn underline_color(&self) -> Option<CellColor>;
    fn attrs(&self) -> CellAttrs;
    fn hyperlink(&self) -> Option<Self::Link>;
    /// Blank with default colors and no visible decoration: what trailing-space trimming skips.
    fn is_empty(&self) -> bool;
}

/// One grid row.
pub(super) trait RowRead {
    type Cell: CellRead;

    /// Every column of the row, left to right.
    fn cells(&self) -> &[Self::Cell];
    /// The row's text continues on the next row (a soft wrap).
    fn wrapped(&self) -> bool;
}

/// Cursor shape as the program asked for it with `DECSCUSR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EngineCursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EngineCursorStyle {
    pub(super) shape: EngineCursorShape,
    pub(super) blinking: bool,
}

/// A grid point on the live screen: `line` counts from the top of the live screen, so history is
/// negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GridPoint {
    pub(super) line: i32,
    pub(super) column: usize,
}

/// The pen the cursor writes with: what a replay has to restore after the grid.
pub(super) struct CursorTemplate<L> {
    pub(super) fg: CellColor,
    pub(super) bg: CellColor,
    pub(super) attrs: CellAttrs,
    pub(super) underline_color: Option<CellColor>,
    pub(super) hyperlink: Option<L>,
}

/// Terminal modes tui-lipan reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ModeFlags(u32);

impl ModeFlags {
    pub(super) const SHOW_CURSOR: Self = Self(1);
    pub(super) const APP_CURSOR: Self = Self(1 << 1);
    pub(super) const APP_KEYPAD: Self = Self(1 << 2);
    pub(super) const MOUSE_REPORT_CLICK: Self = Self(1 << 3);
    pub(super) const BRACKETED_PASTE: Self = Self(1 << 4);
    pub(super) const SGR_MOUSE: Self = Self(1 << 5);
    pub(super) const MOUSE_MOTION: Self = Self(1 << 6);
    pub(super) const LINE_WRAP: Self = Self(1 << 7);
    pub(super) const FOCUS_IN_OUT: Self = Self(1 << 8);
    pub(super) const ALT_SCREEN: Self = Self(1 << 9);
    pub(super) const MOUSE_DRAG: Self = Self(1 << 10);
    pub(super) const UTF8_MOUSE: Self = Self(1 << 11);
    pub(super) const DISAMBIGUATE_ESC_CODES: Self = Self(1 << 12);
    pub(super) const REPORT_EVENT_TYPES: Self = Self(1 << 13);
    pub(super) const REPORT_ALTERNATE_KEYS: Self = Self(1 << 14);
    pub(super) const REPORT_ALL_KEYS_AS_ESC: Self = Self(1 << 15);
    pub(super) const REPORT_ASSOCIATED_TEXT: Self = Self(1 << 16);
    /// `DECSET 1016`, SGR-pixel mouse reports.
    pub(super) const SGR_PIXELS_MOUSE: Self = Self(1 << 17);

    pub(super) const fn empty() -> Self {
        Self(0)
    }

    pub(super) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(super) fn set(&mut self, other: Self, enabled: bool) {
        if enabled {
            self.0 |= other.0;
        } else {
            self.0 &= !other.0;
        }
    }
}

/// What the engine did to its grids while parsing one stretch of bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GridStep {
    /// History lines that fell off the top, each counted once.
    pub(super) evicted: usize,
    /// The child sent `RIS`: both grids were replaced and every line anchor is gone.
    pub(super) reset: bool,
    /// The alternate screen was left (possibly reopened afterwards).
    pub(super) left_alt_screen: bool,
    /// A grid operation completed, as opposed to state-only sequences.
    pub(super) grid_mutated: bool,
}

impl GridStep {
    pub(super) fn merge(&mut self, other: Self) {
        self.evicted += other.evicted;
        self.reset |= other.reset;
        self.left_alt_screen |= other.left_alt_screen;
        self.grid_mutated |= other.grid_mutated;
    }
}

/// Where the observer needs the engine to stop and report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AnchorKind {
    /// Only the position in the reply stream matters.
    Order,
    /// The cursor as the bytes so far left it, committed through any synchronized update.
    Cursor,
}

/// The cursor captured for an [`AnchorKind::Cursor`] anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CursorAnchor {
    /// Absolute retained line (`0` = oldest history line) of the cursor.
    pub(super) absolute_line: usize,
    pub(super) column: usize,
    pub(super) alt_screen: bool,
}

/// What the observer wants done at an anchor; opaque to the engine apart from its kind.
pub(super) trait EngineAnchor {
    fn kind(&self) -> AnchorKind;
}

/// In-band notifications from an engine, in stream order.
pub(super) trait EngineObserver {
    type Anchor: EngineAnchor;

    /// A reply the engine generated for the child (DA, DSR, color queries, ...).
    fn respond(&mut self, reply: Vec<u8>);
    /// The engine finished a stretch of grid work.
    fn grid_step(&mut self, step: GridStep);
    /// `OSC 7`, `OSC 9;9`, `OSC 22` or `OSC 133`, split on `;`.
    fn osc(&mut self, params: &[&[u8]]);
    /// `CSI > q` / `CSI > 0 q` (XTVERSION).
    fn xtversion_query(&mut self);
    /// `DECSTR` or `RIS`: pointer-shape stacks reset.
    fn pointer_reset(&mut self);
    /// `DECSET`/`DECRST` 47, 1047 or 1049.
    fn alternate_screen(&mut self, active: bool);
    /// Whether the notifications so far need an anchor before parsing continues.
    fn wants_anchor(&self) -> bool;
    /// Take the anchor [`Self::wants_anchor`] asked for.
    fn take_anchor(&mut self) -> Option<Self::Anchor>;
    /// Run `anchor` where it landed in the stream; `cursor` is set for [`AnchorKind::Cursor`].
    fn anchor(&mut self, anchor: Self::Anchor, cursor: Option<CursorAnchor>);
}

/// A point to carry through a rewrap: an absolute line and column on one of the two screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TrackedPoint {
    pub(super) absolute_line: usize,
    pub(super) column: u16,
    pub(super) alt_screen: bool,
}

/// The start of the prompt being edited, so a resize can erase and reflow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PromptAnchor {
    pub(super) absolute_line: usize,
    pub(super) column: usize,
}

pub(super) struct ResizeRequest<'a> {
    pub(super) rows: u16,
    pub(super) cols: u16,
    /// The active shell prompt, erased before the reflow so the shell's redraw is not additive.
    pub(super) prompt: Option<PromptAnchor>,
    /// Points to follow through a column change; `None` skips tracking entirely.
    pub(super) track: Option<&'a [TrackedPoint]>,
}

pub(super) struct ResizeOutcome {
    /// History lines that fell off the top.
    pub(super) evicted: usize,
    /// Where the prompt anchor landed, in pre-eviction coordinates; `None` drops it.
    pub(super) prompt: Option<PromptAnchor>,
    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    /// For each tracked point, where it landed, or `None` when it is gone. `None` overall when
    /// the column change moved no text, so line indices still hold.
    pub(super) relocated: Option<Vec<Option<(usize, u16)>>>,
}

/// The VT emulator behind a [`TerminalScreen`](super::TerminalScreen).
///
/// Absolute lines count from the oldest retained history line (`0`) of the active screen; the
/// live screen starts at [`Self::history_size`].
pub(super) trait TerminalEngine: Sized {
    type Link: LinkRead;
    type Row<'a>: RowRead<Cell: CellRead<Link = Self::Link>>
    where
        Self: 'a;

    fn new(config: EngineConfig) -> Self;
    /// Back to a freshly constructed state with the same size and configuration.
    fn reset(&mut self);
    /// Change the cursor used while the child has not set one. Returns whether anything changed.
    fn set_default_cursor_style(&mut self, style: EngineCursorStyle) -> bool;
    /// Colors reported for `OSC 4/10/11 ; ?` queries.
    fn set_palette(&mut self, palette: &TerminalColorPalette);
    /// Size reported for `CSI 14 t` / `CSI 18 t`.
    fn set_report_geometry(&mut self, rows: u16, cols: u16, cell: TerminalCellSize);

    /// Report the sequences in raw PTY `bytes` that this engine cannot report in-band, before any
    /// of them is fed. Returns each anchor with the offset in `bytes` it belongs after; the screen
    /// feeds up to it, possibly in pieces, then calls [`Self::anchor`]. Engines with native
    /// callbacks report everything from [`Self::feed`] and return nothing here.
    fn scan_ahead<O: EngineObserver>(
        &mut self,
        bytes: &[u8],
        observer: &mut O,
    ) -> Vec<(usize, O::Anchor)>;
    /// Run an anchor [`Self::scan_ahead`] returned, with the bytes before it fed.
    fn anchor<O: EngineObserver>(&mut self, anchor: O::Anchor, observer: &mut O) -> GridStep;
    /// Parse `bytes`, notifying `observer` in stream order.
    fn feed<O: EngineObserver>(&mut self, bytes: &[u8], observer: &mut O) -> GridStep;
    /// Inside a synchronized update (`DECSET 2026`) whose output is held back.
    fn sync_pending(&self) -> bool;
    /// Apply what a synchronized update holds back, ending it.
    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    fn commit_sync<O: EngineObserver>(&mut self, observer: &mut O) -> GridStep;
    /// Reopen the synchronized update [`Self::commit_sync`] ended.
    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    fn resume_sync<O: EngineObserver>(&mut self, observer: &mut O) -> GridStep;
    /// Viewport rows changed since the last call.
    fn take_damage(&mut self) -> TerminalDamage;

    fn resize(&mut self, request: ResizeRequest<'_>) -> ResizeOutcome;
    fn screen_lines(&self) -> usize;
    fn columns(&self) -> usize;
    /// History lines retained above the live screen of the active screen.
    fn history_size(&self) -> usize;
    /// How far the viewport is scrolled into history.
    fn display_offset(&self) -> usize;
    /// Scroll the viewport to `offset` (clamped to the history).
    fn scroll_display_to(&mut self, offset: usize);

    fn modes(&self) -> ModeFlags;
    /// Changes whenever any mode does, including those [`ModeFlags`] leaves out.
    fn mode_fingerprint(&self) -> u64;
    /// Where the next character goes.
    fn cursor(&self) -> GridPoint;
    /// Where the cursor is drawn: like [`Self::cursor`], but on the glyph cell of a wide character.
    fn display_cursor(&self) -> GridPoint;
    fn cursor_style(&self) -> EngineCursorStyle;
    /// `OSC 12` cursor color.
    fn cursor_color(&self) -> Option<(u8, u8, u8)>;
    fn cursor_template(&self) -> CursorTemplate<Self::Link>;
    /// `OSC 0/2` title, `None` when unset or reset.
    fn title(&self) -> Option<String>;
    fn bell_count(&self) -> u64;
    /// Pending `OSC 52` stores.
    fn clipboard_event_count(&self) -> usize;
    fn drain_clipboard_events(&mut self) -> Vec<TerminalClipboardEvent>;

    /// A row of the active screen, by absolute line (`< history_size() + screen_lines()`).
    fn row(&self, absolute_line: usize) -> Self::Row<'_>;
    /// Spend one bounded step compressing retained history; call while the terminal is idle.
    /// Returns whether more work remains for this idle period.
    fn compress_idle(&mut self) -> bool {
        false
    }
    /// Whether that row soft-wraps into the next, without reading its cells.
    fn row_wrapped(&self, absolute_line: usize) -> bool {
        self.row(absolute_line).wrapped()
    }
    /// Run `read` with the primary screen readable as the active one, then put things back.
    fn with_primary_screen<R>(&mut self, read: impl FnOnce(&Self) -> R) -> R;
}

/// The engine a screen runs, dispatched once per call.
///
/// Held inline: there is one per screen, and boxing the large Alacritty variant would put an
/// indirection on every call into it.
#[allow(clippy::large_enum_variant)]
pub(super) enum Engine {
    Alacritty(AlacrittyEngine),
    #[cfg(feature = "terminal-ghostty")]
    Ghostty(ghostty::GhosttyEngine),
}

/// Run `$body` with `$engine` bound to the concrete engine, so generic grid reads monomorphize.
macro_rules! with_engine {
    ($engine:expr, $bound:ident => $body:expr) => {
        match $engine {
            $crate::widgets::terminal::screen::engine::Engine::Alacritty($bound) => $body,
            #[cfg(feature = "terminal-ghostty")]
            $crate::widgets::terminal::screen::engine::Engine::Ghostty($bound) => $body,
        }
    };
}
pub(super) use with_engine;

macro_rules! forward {
    ($(fn $name:ident(&self $(, $arg:ident: $ty:ty)*) -> $ret:ty;)*) => {
        $(
            pub(super) fn $name(&self $(, $arg: $ty)*) -> $ret {
                with_engine!(self, engine => engine.$name($($arg),*))
            }
        )*
    };
    ($(fn $name:ident(&mut self $(, $arg:ident: $ty:ty)*) -> $ret:ty;)*) => {
        $(
            pub(super) fn $name(&mut self $(, $arg: $ty)*) -> $ret {
                with_engine!(self, engine => engine.$name($($arg),*))
            }
        )*
    };
}

impl Engine {
    pub(super) fn new(kind: TerminalEngineKind, config: EngineConfig) -> Self {
        match kind {
            TerminalEngineKind::Alacritty => Self::Alacritty(AlacrittyEngine::new(config)),
            #[cfg(feature = "terminal-ghostty")]
            TerminalEngineKind::Ghostty => Self::Ghostty(ghostty::GhosttyEngine::new(config)),
        }
    }

    pub(super) fn feed<O: EngineObserver>(&mut self, bytes: &[u8], observer: &mut O) -> GridStep {
        with_engine!(self, engine => engine.feed(bytes, observer))
    }

    pub(super) fn scan_ahead<O: EngineObserver>(
        &mut self,
        bytes: &[u8],
        observer: &mut O,
    ) -> Vec<(usize, O::Anchor)> {
        with_engine!(self, engine => engine.scan_ahead(bytes, observer))
    }

    pub(super) fn anchor<O: EngineObserver>(
        &mut self,
        anchor: O::Anchor,
        observer: &mut O,
    ) -> GridStep {
        with_engine!(self, engine => engine.anchor(anchor, observer))
    }

    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    pub(super) fn commit_sync<O: EngineObserver>(&mut self, observer: &mut O) -> GridStep {
        with_engine!(self, engine => engine.commit_sync(observer))
    }

    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    pub(super) fn resume_sync<O: EngineObserver>(&mut self, observer: &mut O) -> GridStep {
        with_engine!(self, engine => engine.resume_sync(observer))
    }

    pub(super) fn resize(&mut self, request: ResizeRequest<'_>) -> ResizeOutcome {
        with_engine!(self, engine => engine.resize(request))
    }

    pub(super) fn set_palette(&mut self, palette: &TerminalColorPalette) {
        with_engine!(self, engine => engine.set_palette(palette))
    }

    forward! {
        fn sync_pending(&self) -> bool;
        fn screen_lines(&self) -> usize;
        fn history_size(&self) -> usize;
        fn display_offset(&self) -> usize;
        fn modes(&self) -> ModeFlags;
        fn mode_fingerprint(&self) -> u64;
        fn cursor(&self) -> GridPoint;
        fn display_cursor(&self) -> GridPoint;
        fn cursor_style(&self) -> EngineCursorStyle;
        fn cursor_color(&self) -> Option<(u8, u8, u8)>;
        fn title(&self) -> Option<String>;
        fn bell_count(&self) -> u64;
        fn clipboard_event_count(&self) -> usize;
    }

    forward! {
        fn reset(&mut self) -> ();
        fn compress_idle(&mut self) -> bool;
        fn set_default_cursor_style(&mut self, style: EngineCursorStyle) -> bool;
        fn set_report_geometry(&mut self, rows: u16, cols: u16, cell: TerminalCellSize) -> ();
        fn take_damage(&mut self) -> TerminalDamage;
        fn scroll_display_to(&mut self, offset: usize) -> ();
        fn drain_clipboard_events(&mut self) -> Vec<TerminalClipboardEvent>;
    }

    pub(super) fn alt_screen(&self) -> bool {
        self.modes().contains(ModeFlags::ALT_SCREEN)
    }

    /// Absolute retained lines of the active screen, history included.
    pub(super) fn total_lines(&self) -> usize {
        self.history_size() + self.screen_lines()
    }

    /// The cursor's absolute line, clamped to the retained grid.
    #[cfg_attr(not(feature = "terminal-images"), allow(dead_code))]
    pub(super) fn cursor_absolute_line(&self) -> usize {
        absolute_line(self.history_size(), self.cursor().line)
    }

    /// The scrollback limit the engine was built with.
    #[cfg(test)]
    pub(super) fn scrollback_limit(&self) -> usize {
        match self {
            Self::Alacritty(engine) => engine.scrollback_len,
            #[cfg(feature = "terminal-ghostty")]
            Self::Ghostty(engine) => engine.scrollback_limit(),
        }
    }

    #[cfg(test)]
    pub(super) fn alacritty(&self) -> &AlacrittyEngine {
        match self {
            Self::Alacritty(engine) => engine,
            #[cfg(feature = "terminal-ghostty")]
            Self::Ghostty(_) => panic!("not an Alacritty engine"),
        }
    }

    #[cfg(test)]
    pub(super) fn alacritty_mut(&mut self) -> &mut AlacrittyEngine {
        match self {
            Self::Alacritty(engine) => engine,
            #[cfg(feature = "terminal-ghostty")]
            Self::Ghostty(_) => panic!("not an Alacritty engine"),
        }
    }
}

/// Absolute line of a live-screen-relative `line`, clamped at the oldest retained line.
pub(super) fn absolute_line(history_size: usize, line: i32) -> usize {
    usize::try_from((i64::from(line) + history_size as i64).max(0)).unwrap_or(0)
}

/// The live-screen-relative line of viewport `row` at `display_offset`.
pub(super) fn viewport_line(display_offset: usize, row: usize) -> i32 {
    row as i32 - display_offset as i32
}
