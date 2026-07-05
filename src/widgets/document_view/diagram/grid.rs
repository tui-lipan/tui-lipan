use std::collections::HashMap;

use crate::style::text::Span;
use crate::style::{BorderStyle, Rect, Style};
use crate::widgets::common::box_glyphs::glyph_for_bits;
use crate::widgets::common::simple_diagram::{EndpointGlyph, SimpleDiagramBoxShape};

use super::specs::SequenceMessageSpec;
use super::{SEQUENCE_NOTE_LABEL_PREFIX, SequenceDiagramStyles, StyledDiagramRows};

#[derive(Clone, Copy)]
pub(super) struct StyledCell {
    ch: char,
    style: Style,
}
pub(super) fn cell_is_perpendicular_junction(
    merged: &HashMap<(i16, i16), (u8, bool, Style)>,
    x: i16,
    y: i16,
    dir: EndpointDir,
) -> bool {
    let Some(&(bits, _, _)) = merged.get(&(x, y)) else {
        return false;
    };
    use crate::widgets::common::box_glyphs::{EAST, NORTH, SOUTH, WEST};
    let perpendicular_mask = match dir {
        EndpointDir::Up | EndpointDir::Down => EAST | WEST,
        EndpointDir::Left | EndpointDir::Right => NORTH | SOUTH,
    };
    bits & perpendicular_mask != 0
}

pub(super) fn should_preserve_junction(glyph: EndpointGlyph) -> bool {
    !matches!(glyph, EndpointGlyph::Arrow)
}

/// Direction the endpoint glyph points: into the box from this side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EndpointDir {
    /// Endpoint sits below box, glyph points up into it.
    Up,
    /// Endpoint sits above box, glyph points down into it.
    Down,
    /// Endpoint sits to the right of box, glyph points left into it.
    Left,
    /// Endpoint sits to the left of box, glyph points right into it.
    Right,
}

pub(super) fn outside_endpoint(x: i16, y: i16, rect: Rect) -> (i16, i16, EndpointDir) {
    let left = rect.x;
    let right = rect.x + rect.w as i16 - 1;
    let top = rect.y;
    let bottom = rect.y + rect.h as i16 - 1;
    if y <= top {
        (x, y - 1, EndpointDir::Down)
    } else if y >= bottom {
        (x, y + 1, EndpointDir::Up)
    } else if x <= left {
        (x - 1, y, EndpointDir::Right)
    } else if x >= right {
        (x + 1, y, EndpointDir::Left)
    } else {
        let distances = [
            (y.saturating_sub(top).abs(), (x, top - 1, EndpointDir::Down)),
            (
                y.saturating_sub(bottom).abs(),
                (x, bottom + 1, EndpointDir::Up),
            ),
            (
                x.saturating_sub(left).abs(),
                (left - 1, y, EndpointDir::Right),
            ),
            (
                x.saturating_sub(right).abs(),
                (right + 1, y, EndpointDir::Left),
            ),
        ];
        distances
            .into_iter()
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, endpoint)| endpoint)
            .unwrap_or((x, y, EndpointDir::Right))
    }
}
pub(super) fn draw_box(
    rows: &mut [Vec<StyledCell>],
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    border: BorderStyle,
    style: Style,
) {
    if w == 0 || h == 0 {
        return;
    }
    let (tl, tr, bl, br, horizontal, vertical) = match border {
        BorderStyle::Rounded => ('╭', '╮', '╰', '╯', '─', '│'),
        BorderStyle::Double => ('╔', '╗', '╚', '╝', '═', '║'),
        _ => ('┌', '┐', '└', '┘', '─', '│'),
    };
    let right = x + w as i16 - 1;
    let bottom = y + h as i16 - 1;
    put_char_signed(rows, x, y, tl, style);
    put_char_signed(rows, right, y, tr, style);
    put_char_signed(rows, x, bottom, bl, style);
    put_char_signed(rows, right, bottom, br, style);
    for px in x + 1..right {
        put_char_signed(rows, px, y, horizontal, style);
        put_char_signed(rows, px, bottom, horizontal, style);
    }
    for py in y + 1..bottom {
        put_char_signed(rows, x, py, vertical, style);
        put_char_signed(rows, right, py, vertical, style);
    }
}

pub(super) fn draw_box_shape(
    rows: &mut [Vec<StyledCell>],
    rect: Rect,
    border: BorderStyle,
    shape: SimpleDiagramBoxShape,
    style: Style,
) {
    match shape {
        SimpleDiagramBoxShape::Rect => {
            draw_box(rows, rect.x, rect.y, rect.w, rect.h, border, style);
        }
        SimpleDiagramBoxShape::Cylinder => {
            draw_box(
                rows,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                BorderStyle::Rounded,
                style,
            );
            if rect.w <= 4 || rect.h <= 2 {
                return;
            }

            let left = rect.x;
            let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
            let top_inner = rect.y.saturating_add(1);
            put_char_signed(rows, left, top_inner, '(', style);
            put_char_signed(rows, right, top_inner, ')', style);
            if rect.h > 3 {
                for x in left.saturating_add(1)..right {
                    put_char_signed(rows, x, top_inner, '─', style);
                }
            }
        }
    }
}

pub(super) fn put_endpoint(
    rows: &mut [Vec<StyledCell>],
    x: i16,
    y: i16,
    glyph: EndpointGlyph,
    dir: EndpointDir,
    style: Style,
) {
    let text = match (glyph, dir) {
        (EndpointGlyph::None, _) => return,
        (EndpointGlyph::Arrow, EndpointDir::Right) => "▶",
        (EndpointGlyph::Arrow, EndpointDir::Left) => "◀",
        (EndpointGlyph::Arrow, EndpointDir::Down) => "▼",
        (EndpointGlyph::Arrow, EndpointDir::Up) => "▲",
        (EndpointGlyph::Triangle, EndpointDir::Right) => "▷",
        (EndpointGlyph::Triangle, EndpointDir::Left) => "◁",
        (EndpointGlyph::Triangle, EndpointDir::Down) => "▽",
        (EndpointGlyph::Triangle, EndpointDir::Up) => "△",
        (EndpointGlyph::Diamond, _) => "◆",
        (EndpointGlyph::Circle, _) => "○",
        (EndpointGlyph::CrowZeroOrOne, EndpointDir::Up | EndpointDir::Down) => "○",
        (EndpointGlyph::CrowZeroOrOne, _) => "o|",
        (EndpointGlyph::CrowExactlyOne, EndpointDir::Up | EndpointDir::Down) => "┿",
        (EndpointGlyph::CrowExactlyOne, _) => "||",
        (EndpointGlyph::CrowZeroOrMore, EndpointDir::Up) => "┻",
        (EndpointGlyph::CrowZeroOrMore, EndpointDir::Down) => "┳",
        (EndpointGlyph::CrowZeroOrMore, _) => "}o",
        (EndpointGlyph::CrowOneOrMore, EndpointDir::Up) => "┻",
        (EndpointGlyph::CrowOneOrMore, EndpointDir::Down) => "┳",
        (EndpointGlyph::CrowOneOrMore, _) => "}|",
    };
    put_text_signed(rows, x, y, text, style);
}

pub(super) fn edge_glyph(bits: u8, dashed: bool) -> char {
    if dashed {
        let horizontal = bits
            & (crate::widgets::common::box_glyphs::EAST | crate::widgets::common::box_glyphs::WEST)
            != 0;
        let vertical = bits
            & (crate::widgets::common::box_glyphs::NORTH
                | crate::widgets::common::box_glyphs::SOUTH)
            != 0;
        match (horizontal, vertical) {
            (true, false) => '┄',
            (false, true) => '┆',
            _ => glyph_for_bits(bits, BorderStyle::Plain),
        }
    } else {
        glyph_for_bits(bits, BorderStyle::Plain)
    }
}

pub(super) fn class_relation_glyphs(arrow: &str) -> (EndpointGlyph, EndpointGlyph, bool) {
    match arrow {
        "<|--" => (EndpointGlyph::Triangle, EndpointGlyph::None, false),
        "<|.." => (EndpointGlyph::Triangle, EndpointGlyph::None, true),
        "*--" => (EndpointGlyph::Diamond, EndpointGlyph::None, false),
        "o--" => (EndpointGlyph::Circle, EndpointGlyph::None, false),
        "..>" => (EndpointGlyph::None, EndpointGlyph::Arrow, true),
        "-->" => (EndpointGlyph::None, EndpointGlyph::Arrow, false),
        _ => (EndpointGlyph::None, EndpointGlyph::None, false),
    }
}

pub(super) fn er_cardinality_glyph(cardinality: &str) -> EndpointGlyph {
    match cardinality {
        "|o" | "o|" => EndpointGlyph::CrowZeroOrOne,
        "||" => EndpointGlyph::CrowExactlyOne,
        "}o" | "o{" => EndpointGlyph::CrowZeroOrMore,
        "}|" | "|{" => EndpointGlyph::CrowOneOrMore,
        _ => EndpointGlyph::None,
    }
}
pub(super) fn draw_label_box(
    rows: &mut [Vec<StyledCell>],
    center: usize,
    y: usize,
    label: &str,
    width: usize,
    styles: SequenceDiagramStyles,
) {
    let left = center.saturating_sub(width / 2);
    fill_rect_signed(
        rows,
        Rect {
            x: left.min(i16::MAX as usize) as i16,
            y: y.min(i16::MAX as usize) as i16,
            w: width.min(u16::MAX as usize) as u16,
            h: 3,
        },
        styles.fill,
    );
    put_text(
        rows,
        left,
        y,
        &format!("┌{}┐", "─".repeat(width.saturating_sub(2))),
        styles.border,
    );
    let label_width = unicode_width::UnicodeWidthStr::width(label);
    let inner = width.saturating_sub(2);
    let total_pad = inner.saturating_sub(label_width);
    let pad_left = total_pad / 2;
    let pad_right = total_pad - pad_left;
    put_char(rows, left, y + 1, '│', styles.border);
    put_text(
        rows,
        left + 1,
        y + 1,
        &format!("{:lp$}{label}{:rp$}", "", "", lp = pad_left, rp = pad_right),
        styles.label,
    );
    put_char(
        rows,
        left.saturating_add(width).saturating_sub(1),
        y + 1,
        '│',
        styles.border,
    );
    put_text(
        rows,
        left,
        y + 2,
        &format!("└{}┘", "─".repeat(width.saturating_sub(2))),
        styles.border,
    );
}

pub(super) fn draw_sequence_message(
    rows: &mut [Vec<StyledCell>],
    from: usize,
    to: usize,
    y: usize,
    message: &SequenceMessageSpec,
    styles: SequenceDiagramStyles,
) {
    let label = message.label.as_ref();
    if let Some(note) = label.strip_prefix(SEQUENCE_NOTE_LABEL_PREFIX) {
        draw_sequence_note(rows, from, to, y, note, styles);
        return;
    }
    if from == to {
        put_text(rows, from, y, &format!("↺ {label}"), styles.edge);
        return;
    }
    let (start, end, left_to_right) = if from < to {
        (from, to, true)
    } else {
        (to, from, false)
    };
    let line = if message.dashed { '┄' } else { '─' };
    for x in start..=end {
        put_char(rows, x, y, line, styles.edge);
    }
    let arrow = match (left_to_right, message.open_arrow) {
        (true, true) => '〉',
        (true, false) => '▶',
        (false, true) => '〈',
        (false, false) => '◀',
    };
    put_char(rows, to, y, arrow, styles.edge);
    let label_x =
        start + (end - start).saturating_sub(unicode_width::UnicodeWidthStr::width(label)) / 2;
    put_text(rows, label_x, y.saturating_sub(1), label, styles.edge);
}
pub(super) fn draw_sequence_note(
    rows: &mut [Vec<StyledCell>],
    from: usize,
    to: usize,
    y: usize,
    text: &str,
    styles: SequenceDiagramStyles,
) {
    let note = format!("[ {text} ]");
    let width = unicode_width::UnicodeWidthStr::width(note.as_str());
    let center = (from + to) / 2;
    let x = center.saturating_sub(width / 2);
    for dx in 0..width {
        put_char(rows, x + dx, y, ' ', styles.fill);
    }
    put_char(rows, x, y, '[', styles.border);
    put_char(
        rows,
        x.saturating_add(width).saturating_sub(1),
        y,
        ']',
        styles.border,
    );
    put_text(rows, x.saturating_add(2), y, text, styles.label);
}

pub(super) fn sequence_note_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text) + 4
}

pub(super) fn put_text_signed(
    rows: &mut [Vec<StyledCell>],
    x: i16,
    y: i16,
    text: &str,
    style: Style,
) {
    if x < 0 || y < 0 {
        return;
    }
    for (offset, ch) in text.chars().enumerate() {
        put_char(rows, x as usize + offset, y as usize, ch, style);
    }
}

pub(super) fn clear_label_flanks(
    rows: &mut [Vec<StyledCell>],
    x: i16,
    y: i16,
    text: &str,
    style: Style,
) {
    let width = unicode_width::UnicodeWidthStr::width(text).min(i16::MAX as usize) as i16;
    put_char_signed(rows, x.saturating_sub(1), y, ' ', style);
    put_char_signed(rows, x.saturating_add(width), y, ' ', style);
}

pub(super) fn put_char_signed(
    rows: &mut [Vec<StyledCell>],
    x: i16,
    y: i16,
    ch: char,
    style: Style,
) {
    if x < 0 || y < 0 {
        return;
    }
    put_char(rows, x as usize, y as usize, ch, style);
}

pub(super) fn put_text(rows: &mut [Vec<StyledCell>], x: usize, y: usize, text: &str, style: Style) {
    for (offset, ch) in text.chars().enumerate() {
        put_char(rows, x + offset, y, ch, style);
    }
}

pub(super) fn put_char(rows: &mut [Vec<StyledCell>], x: usize, y: usize, ch: char, style: Style) {
    let Some(row) = rows.get_mut(y) else {
        return;
    };
    if let Some(cell) = row.get_mut(x) {
        *cell = StyledCell { ch, style };
    }
}

pub(super) fn fill_rect_signed(rows: &mut [Vec<StyledCell>], rect: Rect, style: Style) {
    for y in rect.y..rect.y.saturating_add(rect.h as i16) {
        for x in rect.x..rect.x.saturating_add(rect.w as i16) {
            put_char_signed(rows, x, y, ' ', style);
        }
    }
}

pub(super) fn styled_grid(width: usize, height: usize) -> Vec<Vec<StyledCell>> {
    vec![
        vec![
            StyledCell {
                ch: ' ',
                style: Style::default(),
            };
            width
        ];
        height
    ]
}

pub(super) fn plain_rows_to_spans(rows: Vec<String>) -> StyledDiagramRows {
    rows.into_iter()
        .map(|row| vec![Span::new(row)])
        .collect::<Vec<_>>()
}

pub(super) fn trim_styled_rows(rows: Vec<Vec<StyledCell>>) -> StyledDiagramRows {
    rows.into_iter()
        .map(|mut row| {
            let trim_to = row
                .iter()
                .rposition(|cell| cell.ch != ' ' || !cell.style.is_empty())
                .map(|index| index + 1)
                .unwrap_or(0);
            row.truncate(trim_to);
            styled_cells_to_spans(row)
        })
        .collect::<Vec<_>>()
}

pub(super) fn styled_cells_to_spans(cells: Vec<StyledCell>) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut chars = cells.into_iter();
    let Some(first) = chars.next() else {
        return vec![Span::new("")];
    };

    let mut current_style = first.style;
    let mut current_text = String::from(first.ch);
    for cell in chars {
        if cell.style == current_style {
            current_text.push(cell.ch);
        } else {
            spans.push(Span::new(std::mem::take(&mut current_text)).style(current_style));
            current_style = cell.style;
            current_text.push(cell.ch);
        }
    }
    spans.push(Span::new(current_text).style(current_style));
    spans
}
