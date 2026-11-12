//! Replay streams: terminal state written back out as VT bytes, for any engine.

use std::io::{self, Write};

use super::engine::{
    CellAttrs, CellColor, CellRead, CursorTemplate, LinkRead, NamedColor, RowRead, TerminalEngine,
};

/// Write the active screen of `engine` as a repaint: text, styles and hyperlinks.
///
/// `include_scrollback` starts from the oldest history line instead of the top of the live
/// screen. `before_line` runs before each row with the row's index in the written range and
/// whether the previous row ended in a soft wrap.
///
/// No `ED 2` is written: on Alacritty's primary screen it scrolls the cleared viewport into
/// history, adding a phantom scrollback row. The preceding `RIS` (primary) or `DECSET 1049`
/// (alternate) already blanks the target grid.
pub(super) fn write_grid_repaint<E: TerminalEngine, W: Write>(
    engine: &E,
    writer: &mut W,
    include_scrollback: bool,
    mut before_line: impl FnMut(&mut W, usize, bool) -> io::Result<()>,
) -> io::Result<()> {
    let history = engine.history_size();
    let start = if include_scrollback { 0 } else { history };
    let end = history + engine.screen_lines();
    writer.write_all(b"\x1b[0m\x1b[H")?;
    let mut style = ReplayStyle::default();
    let mut hyperlink: Option<E::Link> = None;
    let mut pending_wrap = false;

    for absolute in start..end {
        before_line(writer, absolute - start, pending_wrap)?;
        let row = engine.row(absolute);
        let cells = row.cells();
        let wrapline = row.wrapped();
        pending_wrap = wrapline;
        let end_col = if wrapline {
            cells.len()
        } else {
            cells
                .iter()
                .rposition(|cell| !cell.is_empty() || cell.hyperlink().is_some())
                .map_or(0, |col| col + 1)
        };
        for cell in &cells[..end_col] {
            if cell.attrs().intersects(CellAttrs::SPACERS) {
                continue;
            }
            let next_hyperlink = cell.hyperlink();
            if next_hyperlink != hyperlink {
                write_replay_hyperlink(writer, next_hyperlink.as_ref())?;
                hyperlink = next_hyperlink;
            }
            let next_style = ReplayStyle::of_cell(cell);
            if next_style != style {
                next_style.write_sgr(writer)?;
                style = next_style;
            }
            write_cell_text(writer, cell)?;
        }
        if absolute + 1 != end && !wrapline {
            if style != ReplayStyle::default() {
                ReplayStyle::default().write_sgr(writer)?;
                style = ReplayStyle::default();
            }
            writer.write_all(b"\r\n")?;
        }
    }
    if hyperlink.is_some() {
        write_replay_hyperlink::<_, E::Link>(writer, None)?;
    }
    writer.write_all(b"\x1b[0m")
}

/// Restore the cursor position and its pen.
pub(super) fn write_cursor<E: TerminalEngine, W: Write>(
    engine: &E,
    writer: &mut W,
    rows: u16,
    cols: u16,
) -> io::Result<()> {
    let cursor = engine.cursor();
    let row = (cursor.line.max(0) as usize + 1).min(rows as usize);
    let col = (cursor.column + 1).min(cols as usize);
    write!(writer, "\x1b[{row};{col}H")?;
    let template = engine.cursor_template();
    ReplayStyle::of_template(&template).write_sgr(writer)?;
    write_replay_hyperlink(writer, template.hyperlink.as_ref())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReplayStyle {
    fg: CellColor,
    bg: CellColor,
    attrs: CellAttrs,
    underline_color: Option<CellColor>,
}

impl Default for ReplayStyle {
    fn default() -> Self {
        Self {
            fg: CellColor::FOREGROUND,
            bg: CellColor::BACKGROUND,
            attrs: CellAttrs::empty(),
            underline_color: None,
        }
    }
}

/// The attributes a replay reproduces.
const REPLAY_ATTRS: CellAttrs = CellAttrs::from_bits_truncate(
    CellAttrs::BOLD.bits()
        | CellAttrs::DIM.bits()
        | CellAttrs::ITALIC.bits()
        | CellAttrs::ALL_UNDERLINES.bits()
        | CellAttrs::INVERSE.bits()
        | CellAttrs::HIDDEN.bits()
        | CellAttrs::STRIKEOUT.bits(),
);

impl ReplayStyle {
    fn of_cell(cell: &impl CellRead) -> Self {
        Self {
            fg: cell.fg(),
            bg: cell.bg(),
            attrs: cell.attrs().intersection(REPLAY_ATTRS),
            underline_color: cell.underline_color(),
        }
    }

    fn of_template<L>(template: &CursorTemplate<L>) -> Self {
        Self {
            fg: template.fg,
            bg: template.bg,
            attrs: template.attrs.intersection(REPLAY_ATTRS),
            underline_color: template.underline_color,
        }
    }

    fn write_sgr<W: Write>(self, writer: &mut W) -> io::Result<()> {
        let mut params = vec!["0".to_string()];
        let attrs = self.attrs;
        if attrs.contains(CellAttrs::BOLD) {
            params.push("1".to_string());
        }
        if attrs.contains(CellAttrs::DIM) {
            params.push("2".to_string());
        }
        if attrs.contains(CellAttrs::ITALIC) {
            params.push("3".to_string());
        }
        if attrs.contains(CellAttrs::DOUBLE_UNDERLINE) {
            params.push("4:2".to_string());
        } else if attrs.contains(CellAttrs::UNDERCURL) {
            params.push("4:3".to_string());
        } else if attrs.contains(CellAttrs::DOTTED_UNDERLINE) {
            params.push("4:4".to_string());
        } else if attrs.contains(CellAttrs::DASHED_UNDERLINE) {
            params.push("4:5".to_string());
        } else if attrs.contains(CellAttrs::UNDERLINE) {
            params.push("4".to_string());
        }
        if attrs.contains(CellAttrs::INVERSE) {
            params.push("7".to_string());
        }
        if attrs.contains(CellAttrs::HIDDEN) {
            params.push("8".to_string());
        }
        if attrs.contains(CellAttrs::STRIKEOUT) {
            params.push("9".to_string());
        }
        push_color_sgr(&mut params, self.fg, true);
        push_color_sgr(&mut params, self.bg, false);
        if let Some(color) = self.underline_color {
            push_underline_color_sgr(&mut params, color);
        }
        write!(writer, "\x1b[{}m", params.join(";"))
    }
}

pub(super) fn write_dec_mode<W: Write>(writer: &mut W, mode: u16, enabled: bool) -> io::Result<()> {
    let suffix = if enabled { 'h' } else { 'l' };
    write!(writer, "\x1b[?{mode}{suffix}")
}

fn write_cell_text<W: Write>(writer: &mut W, cell: &impl CellRead) -> io::Result<()> {
    let mut buf = [0; 4];
    writer.write_all(cell.ch().encode_utf8(&mut buf).as_bytes())?;
    for ch in cell.zerowidth() {
        writer.write_all(ch.encode_utf8(&mut buf).as_bytes())?;
    }
    Ok(())
}

fn write_replay_hyperlink<W: Write, L: LinkRead>(
    writer: &mut W,
    hyperlink: Option<&L>,
) -> io::Result<()> {
    writer.write_all(b"\x1b]8;;")?;
    if let Some(hyperlink) = hyperlink {
        writer.write_all(hyperlink.uri().as_bytes())?;
    }
    writer.write_all(b"\x1b\\")
}

fn push_color_sgr(params: &mut Vec<String>, color: CellColor, foreground: bool) {
    match color {
        CellColor::Named(named) => {
            let value = named_color_sgr(named, foreground);
            params.push(value.to_string());
        }
        CellColor::Indexed(index) => {
            params.push(if foreground { "38" } else { "48" }.to_string());
            params.push("5".to_string());
            params.push(index.to_string());
        }
        CellColor::Rgb(r, g, b) => {
            params.push(if foreground { "38" } else { "48" }.to_string());
            params.push("2".to_string());
            params.push(r.to_string());
            params.push(g.to_string());
            params.push(b.to_string());
        }
    }
}

fn push_underline_color_sgr(params: &mut Vec<String>, color: CellColor) {
    match color {
        CellColor::Named(named) => {
            if let Some(index) = named_color_index(named) {
                params.push("58".to_string());
                params.push("5".to_string());
                params.push(index.to_string());
            }
        }
        CellColor::Indexed(index) => {
            params.push("58".to_string());
            params.push("5".to_string());
            params.push(index.to_string());
        }
        CellColor::Rgb(r, g, b) => {
            params.push("58".to_string());
            params.push("2".to_string());
            params.push(r.to_string());
            params.push(g.to_string());
            params.push(b.to_string());
        }
    }
}

fn named_color_sgr(color: NamedColor, foreground: bool) -> u16 {
    let ansi = |base: u16| if foreground { base } else { base + 10 };
    match color {
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => 39,
        NamedColor::Background => 49,
        NamedColor::Cursor => ansi(39),
        _ => match named_color_index(color) {
            Some(index @ 0..=7) => ansi(30 + u16::from(index)),
            Some(index) => ansi(90 + u16::from(index - 8)),
            None => ansi(39),
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The compact SGR table must match the one it replaced, entry by entry.
    #[test]
    fn named_color_sgr_matches_the_ansi_table() {
        let table = [
            (NamedColor::Black, 30),
            (NamedColor::Red, 31),
            (NamedColor::Green, 32),
            (NamedColor::Yellow, 33),
            (NamedColor::Blue, 34),
            (NamedColor::Magenta, 35),
            (NamedColor::Cyan, 36),
            (NamedColor::White, 37),
            (NamedColor::DimBlack, 30),
            (NamedColor::DimRed, 31),
            (NamedColor::DimGreen, 32),
            (NamedColor::DimYellow, 33),
            (NamedColor::DimBlue, 34),
            (NamedColor::DimMagenta, 35),
            (NamedColor::DimCyan, 36),
            (NamedColor::DimWhite, 37),
            (NamedColor::BrightBlack, 90),
            (NamedColor::BrightRed, 91),
            (NamedColor::BrightGreen, 92),
            (NamedColor::BrightYellow, 93),
            (NamedColor::BrightBlue, 94),
            (NamedColor::BrightMagenta, 95),
            (NamedColor::BrightCyan, 96),
            (NamedColor::BrightWhite, 97),
            (NamedColor::Cursor, 39),
        ];
        for (color, foreground) in table {
            assert_eq!(named_color_sgr(color, true), foreground, "{color:?}");
            assert_eq!(named_color_sgr(color, false), foreground + 10, "{color:?}");
        }
        for color in [
            NamedColor::Foreground,
            NamedColor::BrightForeground,
            NamedColor::DimForeground,
        ] {
            assert_eq!(named_color_sgr(color, true), 39);
            assert_eq!(named_color_sgr(color, false), 39);
        }
        assert_eq!(named_color_sgr(NamedColor::Background, true), 49);
        assert_eq!(named_color_sgr(NamedColor::Background, false), 49);
    }
}
