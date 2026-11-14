//! Cell positions for live rows that cannot safely be remeasured as joined text.

use super::*;
use unicode_width::UnicodeWidthChar;

pub(super) fn render_row<E: TerminalEngine>(
    engine: &E,
    row: usize,
    palette: &TerminalColorPalette,
    decorations: &[TerminalDecoration],
) -> Option<Vec<(u16, u16, Span)>> {
    let absolute = viewport_row_line(engine, engine.display_offset(), row)?;
    let grid = engine.row(absolute);
    let mut cells = grid
        .cells()
        .iter()
        .enumerate()
        .filter_map(|(col, cell)| {
            let attrs = cell.attrs();
            if attrs.intersects(CellAttrs::SPACERS) {
                return None;
            }
            let width = if attrs.contains(CellAttrs::WIDE_CHAR) {
                2
            } else {
                1
            };
            let mut text = String::new();
            push_cell_text(&mut text, cell, attrs);
            let style = style_from_mapped(
                map_term_color(cell.fg(), palette),
                map_term_color(cell.bg(), palette),
                style_run_bits(attrs),
            );
            Some((col as u16, width, Span::new(text).style(style)))
        })
        .collect();
    decorate_cell_row(&mut cells, row, decorations);
    Some(cells)
}

pub(super) fn needs_cell_layout(cell: &impl CellRead, attrs: CellAttrs) -> bool {
    let ch = cell.ch();
    let combined = !cell.zerowidth().is_empty();
    if matches!(ch, ' '..='~') && !combined {
        return false;
    }
    let width = if attrs.contains(CellAttrs::WIDE_CHAR) {
        2
    } else {
        1
    };
    combined
        || matches!(ch, '\u{1f3fb}'..='\u{1f3ff}')
        || ch.width().is_some_and(|measured| measured != width)
}

fn decorate_cell_row(
    cells: &mut Vec<(u16, u16, Span)>,
    row: usize,
    decorations: &[TerminalDecoration],
) {
    let mut decorations: Vec<_> = decorations.iter().filter(|d| d.row == row).collect();
    decorations.sort_by_key(|d| d.start_col);
    for decoration in &decorations {
        if decoration.text.is_none() {
            for (col, width, span) in cells.iter_mut() {
                if usize::from(*col) < decoration.end_col
                    && usize::from(col.saturating_add(*width)) > decoration.start_col
                {
                    span.style = span.style.patch(decoration.style);
                }
            }
        }
    }
    for decoration in &decorations {
        if let Some(DecorationText::Overlay(span)) = &decoration.text {
            overlay_cell_label(cells, decoration.start_col, span, false);
        }
    }
    for decoration in decorations.iter().rev() {
        if let Some(DecorationText::Insert(span)) = &decoration.text {
            overlay_cell_label(cells, decoration.start_col, span, true);
        }
    }
}

fn overlay_cell_label(cells: &mut Vec<(u16, u16, Span)>, col: usize, span: &Span, insert: bool) {
    let start = if insert {
        insertion_column(cells, col)
    } else {
        let Ok(col) = u16::try_from(col) else { return };
        col
    };
    let (end, label) = position_label(cells, start, span, insert);
    if start == end {
        return;
    }
    let mut kept = Vec::new();
    for (col, width, cell) in cells.drain(..) {
        if insert && col >= start {
            kept.push((col.saturating_add(end - start), width, cell));
        } else if !insert && col < end && col.saturating_add(width) > start {
            keep_uncovered_columns(&mut kept, (col, width, cell), start..end);
        } else {
            kept.push((col, width, cell));
        }
    }
    *cells = kept;
    cells.extend(label);
    cells.sort_by_key(|(col, _, _)| *col);
}

/// Match `insert_at_column`: a cut into a wide cell inserts before the whole glyph.
fn insertion_column(cells: &[(u16, u16, Span)], requested: usize) -> u16 {
    let end = cells
        .last()
        .map_or(0, |(col, width, _)| col.saturating_add(*width));
    let requested = requested.min(usize::from(end)) as u16;
    cells
        .iter()
        .find(|(col, width, _)| *col <= requested && requested < col.saturating_add(*width))
        .map_or(requested, |(col, _, _)| *col)
}

fn position_label(
    cells: &mut [(u16, u16, Span)],
    start: u16,
    span: &Span,
    insert: bool,
) -> (u16, Vec<(u16, u16, Span)>) {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let mut end = start;
    let mut label = Vec::new();
    for text in span.content.graphemes(true) {
        let width = text.width().min(u16::MAX as usize) as u16;
        if width == 0 {
            if insert {
                append_zero_width(cells, &mut label, end, text);
            }
        } else {
            label.push((end, width, Span::new(text.to_owned()).style(span.style)));
            end = end.saturating_add(width);
        }
    }
    (end, label)
}

fn append_zero_width(
    cells: &mut [(u16, u16, Span)],
    label: &mut [(u16, u16, Span)],
    col: u16,
    text: &str,
) {
    let previous = label.last_mut().or_else(|| {
        cells
            .iter_mut()
            .find(|(start, width, _)| start.saturating_add(*width) == col)
    });
    if let Some((_, _, previous)) = previous {
        previous.content = format!("{}{text}", previous.content).into();
    }
}

/// A label covering half a wide glyph leaves the uncovered half blank.
fn keep_uncovered_columns(
    kept: &mut Vec<(u16, u16, Span)>,
    (col, width, cell): (u16, u16, Span),
    covered: std::ops::Range<u16>,
) {
    for x in col..col.saturating_add(width) {
        if !covered.contains(&x) {
            kept.push((x, 1, Span::new(" ").style(cell.style)));
        }
    }
}
