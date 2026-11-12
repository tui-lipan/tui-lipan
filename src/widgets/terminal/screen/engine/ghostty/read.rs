//! Grid reads for the Ghostty engine: rows, cells and hyperlinks decoded into owned values, and
//! viewport damage from the render state.
//!
//! # Decoding
//!
//! [`row`] resolves its row once ([`Terminal::grid_row`]) and reads one packed cell per column,
//! decoded with the bit layout of the linked library (`ghostty/cell.rs`; a test pins it to the
//! library version, and a mismatch at run time falls back to the library's accessors). Styles are
//! looked up only when the style id changes along the row, graphemes only for cells holding more
//! than one codepoint, and URIs only for linked cells, with consecutive cells of one link sharing
//! its allocation.
//!
//! Rows of the live screen are read from a copy of their packed cells that [`take_damage`] keeps
//! from the render state (`ROW_DATA_CELLS_RAW`), which saves a library call per cell: about a
//! quarter of a full-repaint snapshot. Every mutation not followed by [`take_damage`] must call
//! [`ReadScratch::invalidate`]; reads then fall back to the grid until the next copy.
//!
//! Decoded live rows are kept as well, shared through `Rc`, until [`take_damage`] sees their row
//! change: a snapshot that rebuilds every row without the terminal changing (a palette or cell
//! size change) decodes nothing.
//!
//! # Fidelity to the Alacritty representation
//!
//! - **Named vs indexed colors.** libghostty-vt stores `SGR 31` and `SGR 38;5;1` the same way,
//!   as palette index 1, and its C API exposes nothing that tells them apart. Palette indices
//!   0-15 read as [`CellColor::Named`], what `SGR 30-37`/`90-97` write; 16-255 as
//!   [`CellColor::Indexed`]. Underline colors (`SGR 58`) have no named form and stay indexed.
//! - **Link identity.** The C API exposes a link's URI but not its `id=`, so links compare by
//!   URI.
//! - **Empty cells.** [`CellRead::is_empty`] applies Alacritty's rule to the decoded fields, so a
//!   never-written cell and a written space read the same, as in Alacritty.
//! - **Tabs.** Alacritty stores a literal tab where one starts; Ghostty stores nothing there.
//! - **Blink and overline** have no [`CellAttrs`] bit and are dropped, as in Alacritty.

use std::rc::Rc;

use crate::widgets::terminal::damage::{DamageAccumulator, TerminalDamage};
use crate::widgets::terminal::ghostty::cell::{
    CONTENT_BG_PALETTE, CONTENT_BG_RGB, CONTENT_GRAPHEME, Decoded, WIDE_SPACER_HEAD,
    WIDE_SPACER_TAIL, WIDE_WIDE, decode_ffi, decode_packed, packed_layout_ok,
};
use crate::widgets::terminal::ghostty::{
    Dirty, GridRow, Point, RenderState, Style, StyleColor, Terminal, Underline,
};

use super::super::{CellAttrs, CellColor, CellRead, LinkRead, NamedColor, RowRead};

/// Reusable buffers for [`row`]. Owned by the engine, one per terminal.
#[derive(Debug, Default)]
pub(in super::super::super) struct ReadScratch {
    codepoints: Vec<u32>,
    uri: Vec<u8>,
    /// Style ids resolved on the current row. Ids are only unique within a page, and a row never
    /// spans pages, so this is cleared per row.
    styles: Vec<(u16, ResolvedStyle)>,
    /// Packed cells of the live screen's rows, as of the last [`take_damage`].
    viewport: Vec<Vec<u64>>,
    /// Whether [`Self::viewport`] still matches the terminal.
    viewport_valid: bool,
    /// Live rows already decoded from [`Self::viewport`], until their row changes.
    decoded: Vec<Option<GhosttyRow>>,
}

impl ReadScratch {
    /// The terminal changed without [`take_damage`] following: stop reading the copy.
    pub(in super::super::super) fn invalidate(&mut self) {
        self.viewport_valid = false;
    }
}

/// A hyperlink on a Ghostty cell. Ghostty does not expose the OSC 8 id, so links compare by URI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super::super) struct GhosttyLink {
    /// Thin, so a cell without a link pays one word for it.
    uri: Rc<Box<str>>,
}

impl LinkRead for GhosttyLink {
    fn uri(&self) -> &str {
        &self.uri
    }
}

/// A Ghostty cell, decoded from its page.
#[derive(Clone, Debug)]
pub(in super::super::super) struct GhosttyCell {
    /// The base character; `' '` for a never-written cell.
    ch: char,
    /// Combining characters, boxed: rare, and a thin pointer keeps the cell small.
    #[allow(clippy::box_collection)] // one word instead of a slice's two
    zerowidth: Option<Box<Vec<char>>>,
    fg: CellColor,
    bg: CellColor,
    underline_color: Option<CellColor>,
    attrs: CellAttrs,
    hyperlink: Option<GhosttyLink>,
    /// The last cell of a soft-wrapped row, where Alacritty keeps the wrap flag: never empty.
    wrap_tail: bool,
}

impl GhosttyCell {
    const BLANK: Self = Self {
        ch: ' ',
        zerowidth: None,
        fg: CellColor::FOREGROUND,
        bg: CellColor::BACKGROUND,
        underline_color: None,
        attrs: CellAttrs::empty(),
        hyperlink: None,
        wrap_tail: false,
    };
}

impl CellRead for GhosttyCell {
    type Link = GhosttyLink;

    fn ch(&self) -> char {
        self.ch
    }

    fn zerowidth(&self) -> &[char] {
        self.zerowidth.as_deref().map_or(&[], Vec::as_slice)
    }

    fn fg(&self) -> CellColor {
        self.fg
    }

    fn bg(&self) -> CellColor {
        self.bg
    }

    fn underline_color(&self) -> Option<CellColor> {
        self.underline_color
    }

    fn attrs(&self) -> CellAttrs {
        self.attrs
    }

    fn hyperlink(&self) -> Option<GhosttyLink> {
        self.hyperlink.clone()
    }

    /// Alacritty's rule: a space with default colors and no inverse, underline, strikeout,
    /// spacer or wrap flag, or combining mark.
    fn is_empty(&self) -> bool {
        !self.wrap_tail
            && (self.ch == ' ' || self.ch == '\t')
            && self.bg == CellColor::BACKGROUND
            && self.fg == CellColor::FOREGROUND
            && !self.attrs.intersects(
                CellAttrs::INVERSE
                    | CellAttrs::ALL_UNDERLINES
                    | CellAttrs::STRIKEOUT
                    | CellAttrs::SPACERS,
            )
            && self.zerowidth.is_none()
    }
}

/// A Ghostty row, decoded into owned cells. Cloning shares them.
#[derive(Clone, Debug)]
pub(in super::super::super) struct GhosttyRow {
    cells: Rc<[GhosttyCell]>,
    wrapped: bool,
}

impl RowRead for GhosttyRow {
    type Cell = GhosttyCell;

    fn cells(&self) -> &[GhosttyCell] {
        &self.cells
    }

    fn wrapped(&self) -> bool {
        self.wrapped
    }
}

/// A cell style converted to what [`GhosttyCell`] stores.
#[derive(Clone, Copy, Debug)]
struct ResolvedStyle {
    fg: CellColor,
    bg: CellColor,
    underline_color: Option<CellColor>,
    attrs: CellAttrs,
}

impl ResolvedStyle {
    const DEFAULT: Self = Self {
        fg: CellColor::FOREGROUND,
        bg: CellColor::BACKGROUND,
        underline_color: None,
        attrs: CellAttrs::empty(),
    };

    fn new(style: &Style) -> Self {
        let flag = |on: bool, attr: CellAttrs| if on { attr } else { CellAttrs::empty() };
        let underline = match style.underline {
            Underline::None => CellAttrs::empty(),
            Underline::Single => CellAttrs::UNDERLINE,
            Underline::Double => CellAttrs::DOUBLE_UNDERLINE,
            Underline::Curly => CellAttrs::UNDERCURL,
            Underline::Dotted => CellAttrs::DOTTED_UNDERLINE,
            Underline::Dashed => CellAttrs::DASHED_UNDERLINE,
        };
        Self {
            fg: style_color(style.fg, NamedColor::Foreground),
            bg: style_color(style.bg, NamedColor::Background),
            underline_color: match style.underline_color {
                StyleColor::Default => None,
                // SGR 58 has no named form.
                StyleColor::Palette(index) => Some(CellColor::Indexed(index)),
                StyleColor::Rgb(rgb) => Some(CellColor::Rgb(rgb.r, rgb.g, rgb.b)),
            },
            attrs: flag(style.bold, CellAttrs::BOLD)
                | flag(style.italic, CellAttrs::ITALIC)
                | flag(style.faint, CellAttrs::DIM)
                | flag(style.inverse, CellAttrs::INVERSE)
                | flag(style.invisible, CellAttrs::HIDDEN)
                | flag(style.strikethrough, CellAttrs::STRIKEOUT)
                | underline,
        }
    }
}

/// Map a palette index the way Alacritty stores the same SGR: `0..16` as named colors.
///
/// Ghostty stores `SGR 31` and `SGR 38;5;1` alike, as palette index 1; the more common form wins.
fn palette_color(index: u8) -> CellColor {
    NAMED
        .get(usize::from(index))
        .map_or(CellColor::Indexed(index), |named| CellColor::Named(*named))
}

pub(super) fn style_color(color: StyleColor, default: NamedColor) -> CellColor {
    match color {
        StyleColor::Default => CellColor::Named(default),
        StyleColor::Rgb(rgb) => CellColor::Rgb(rgb.r, rgb.g, rgb.b),
        StyleColor::Palette(index) => palette_color(index),
    }
}

const NAMED: [NamedColor; 16] = [
    NamedColor::Black,
    NamedColor::Red,
    NamedColor::Green,
    NamedColor::Yellow,
    NamedColor::Blue,
    NamedColor::Magenta,
    NamedColor::Cyan,
    NamedColor::White,
    NamedColor::BrightBlack,
    NamedColor::BrightRed,
    NamedColor::BrightGreen,
    NamedColor::BrightYellow,
    NamedColor::BrightBlue,
    NamedColor::BrightMagenta,
    NamedColor::BrightCyan,
    NamedColor::BrightWhite,
];

/// Read row `screen_y` of the active screen, in Ghostty's screen coordinates (`0` = oldest
/// retained line), `cols` cells wide. A row out of range reads as blank.
pub(in super::super::super) fn row(
    terminal: &Terminal,
    screen_y: u32,
    cols: u16,
    scratch: &mut ReadScratch,
) -> GhosttyRow {
    let live = scratch
        .viewport_valid
        .then_some(screen_y as usize)
        .and_then(|y| y.checked_sub(terminal.scrollback_rows()))
        .filter(|&row| row < scratch.viewport.len());
    if let Some(row) = live
        && let Some(Some(decoded)) = scratch.decoded.get(row)
        && decoded.cells.len() == usize::from(cols)
    {
        return decoded.clone();
    }
    let decoded = decode(terminal, screen_y, cols, live, scratch);
    if let Some(row) = live {
        scratch.decoded.resize(scratch.viewport.len(), None);
        scratch.decoded[row] = Some(decoded.clone());
    }
    decoded
}

/// Decode row `screen_y`, from the copy of live row `live` when there is one.
fn decode(
    terminal: &Terminal,
    screen_y: u32,
    cols: u16,
    live: Option<usize>,
    scratch: &mut ReadScratch,
) -> GhosttyRow {
    let mut cells = Vec::with_capacity(usize::from(cols));
    let mut wrapped = false;
    if let Some(grid) = terminal.grid_row(Point::Screen { x: 0, y: screen_y }) {
        wrapped = grid.wraps();
        let width = cols.min(grid.cols());
        let live = live.filter(|&row| scratch.viewport[row].len() >= usize::from(width));
        // Branch once per row so the common, packed decoder inlines into the cell loop.
        match (live, packed_layout_ok()) {
            (Some(row), true) => {
                let raw = std::mem::take(&mut scratch.viewport[row]);
                decode_row(
                    &grid,
                    width,
                    |x| raw[usize::from(x)],
                    decode_packed,
                    scratch,
                    &mut cells,
                );
                scratch.viewport[row] = raw;
            }
            (None, true) => {
                decode_row(
                    &grid,
                    width,
                    |x| grid.raw(x),
                    decode_packed,
                    scratch,
                    &mut cells,
                );
            }
            (_, false) => {
                decode_row(
                    &grid,
                    width,
                    |x| grid.raw(x),
                    decode_ffi,
                    scratch,
                    &mut cells,
                );
            }
        }
    }
    cells.resize(usize::from(cols), GhosttyCell::BLANK);
    if wrapped && let Some(last) = cells.last_mut() {
        last.wrap_tail = true;
    }
    GhosttyRow {
        cells: cells.into(),
        wrapped,
    }
}

/// Decodes the first `cols` cells of `grid` into `cells`.
fn decode_row(
    grid: &GridRow<'_>,
    cols: u16,
    raw: impl Fn(u16) -> u64,
    decode: impl Fn(u64) -> Decoded,
    scratch: &mut ReadScratch,
    cells: &mut Vec<GhosttyCell>,
) {
    scratch.styles.clear();
    // The previous cell's style id and what it resolved to: neighbors usually share it.
    let mut last: Option<(u16, ResolvedStyle)> = None;
    let mut link: Option<GhosttyLink> = None;
    for x in 0..cols {
        let cell = decode(raw(x));
        let style = match last {
            Some((id, style)) if id == cell.style_id => style,
            _ => {
                let style = resolve_style(grid, scratch, x, cell.style_id);
                last = Some((cell.style_id, style));
                style
            }
        };
        // Everything is computed first and the cell written once: filling it in field by field
        // stalls on the store-to-load forwarding of the copy into the row.
        let (ch, zerowidth) = if cell.tag == CONTENT_GRAPHEME {
            scratch.codepoints.clear();
            grid.graphemes(x, &mut scratch.codepoints);
            let mut chars = scratch.codepoints.iter().filter_map(|&c| char::from_u32(c));
            let ch = chars.next().unwrap_or(' ');
            let rest: Vec<char> = chars.collect();
            (ch, (!rest.is_empty()).then(|| Box::new(rest)))
        } else if cell.tag != CONTENT_BG_PALETTE
            && cell.tag != CONTENT_BG_RGB
            && cell.codepoint != 0
        {
            (char::from_u32(cell.codepoint).unwrap_or(' '), None)
        } else {
            (' ', None)
        };
        let hyperlink = if cell.hyperlink && grid.hyperlink_uri(x, &mut scratch.uri) {
            // Consecutive cells of one link share its allocation.
            link = match link.take() {
                Some(prev) if prev.uri.as_bytes() == scratch.uri.as_slice() => Some(prev),
                _ => Some(GhosttyLink {
                    uri: Rc::new(String::from_utf8_lossy(&scratch.uri).into()),
                }),
            };
            link.clone()
        } else {
            None
        };
        cells.push(GhosttyCell {
            ch,
            zerowidth,
            fg: style.fg,
            bg: match cell.tag {
                CONTENT_BG_PALETTE => palette_color(cell.palette_index),
                CONTENT_BG_RGB => CellColor::Rgb(cell.rgb.r, cell.rgb.g, cell.rgb.b),
                _ => style.bg,
            },
            underline_color: style.underline_color,
            attrs: style.attrs
                | match cell.wide {
                    WIDE_WIDE => CellAttrs::WIDE_CHAR,
                    WIDE_SPACER_TAIL => CellAttrs::WIDE_CHAR_SPACER,
                    WIDE_SPACER_HEAD => CellAttrs::LEADING_WIDE_CHAR_SPACER,
                    _ => CellAttrs::empty(),
                },
            hyperlink,
            wrap_tail: false,
        });
    }
}

fn resolve_style(grid: &GridRow<'_>, scratch: &mut ReadScratch, x: u16, id: u16) -> ResolvedStyle {
    if id == 0 {
        return ResolvedStyle::DEFAULT;
    }
    if let Some((_, style)) = scratch.styles.iter().find(|(known, _)| *known == id) {
        return *style;
    }
    let style = ResolvedStyle::new(&grid.style(x));
    scratch.styles.push((id, style));
    style
}

/// Viewport rows changed since the last call, from the render state's dirty flags. Each row is
/// reported whole: Ghostty tracks rows, not columns.
///
/// The render state must be the engine's only one: an update consumes the terminal's dirty
/// state, so a second render state would see nothing.
///
/// Also brings the copy of the live screen's packed cells in `scratch` up to date: the dirty rows,
/// or every row after a full change or an [`ReadScratch::invalidate`].
pub(in super::super::super) fn take_damage(
    render: &mut RenderState,
    terminal: &mut Terminal,
    scratch: &mut ReadScratch,
) -> TerminalDamage {
    let mut damage = DamageAccumulator::default();
    let (cols, rows) = (terminal.cols(), terminal.rows());
    let dirty = render.update(terminal);
    let copy_all = !scratch.viewport_valid || !matches!(dirty, Ok(Dirty::Clean | Dirty::Partial));
    scratch.viewport.resize_with(usize::from(rows), Vec::new);
    let mut iter = render.rows();
    while let Some(row) = iter.next() {
        let Ok(y) = usize::try_from(row.viewport_y()) else {
            continue;
        };
        let dirty_row = row.is_dirty();
        if dirty_row && matches!(dirty, Ok(Dirty::Partial)) {
            damage.add_row(y, 0, usize::from(cols.saturating_sub(1)), usize::from(rows));
        }
        if (copy_all || dirty_row)
            && let Some(copy) = scratch.viewport.get_mut(y)
        {
            copy.clear();
            copy.extend_from_slice(row.raw_cells());
            if let Some(decoded) = scratch.decoded.get_mut(y) {
                *decoded = None;
            }
        }
    }
    scratch.viewport_valid = dirty.is_ok();
    if !matches!(dirty, Ok(Dirty::Clean | Dirty::Partial)) {
        damage.mark_full();
    }
    render.clean();
    damage.take()
}

#[cfg(test)]
mod tests;
