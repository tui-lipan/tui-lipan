//! Drag handling for sliders, progress bars, text areas, and inputs.

use std::collections::BTreeMap;
use std::sync::Arc;

use unicode_width::UnicodeWidthStr;

use crate::callback::Callback;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::DocumentSelectEvent;
use crate::widgets::document_view::node::{DocumentTableRectSelection, VisualLineKind};
use crate::widgets::table::table_border_glyphs;

use super::*;
struct TableCellHitColParams<'a> {
    visual_col: usize,
    widths: &'a [u16],
    alignments: &'a [crate::widgets::ColumnAlign],
    cell_line_texts: &'a [Arc<str>],
    cell_padding: u16,
    border_variant: crate::style::BorderStyle,
    outer_frame: bool,
    column_separators: bool,
    clamp: bool,
}

fn table_cell_hit_at_col(
    params: TableCellHitColParams<'_>,
) -> Option<(usize, usize, usize, usize)> {
    let TableCellHitColParams {
        visual_col,
        widths,
        alignments,
        cell_line_texts,
        cell_padding,
        border_variant,
        outer_frame,
        column_separators,
        clamp,
    } = params;
    let glyphs = table_border_glyphs(border_variant);

    // Helper: compute cell hit for a specific cell index at a given visual column.
    let hit_cell = |cell_idx: usize,
                    vis_col: usize,
                    byte_cur: usize|
     -> Option<(usize, usize, usize, usize)> {
        let w = *widths.get(cell_idx)? as usize;
        let cell_start_col = {
            let mut c = if outer_frame { 1 } else { 0 };
            for j in 0..cell_idx {
                c += widths[j] as usize;
                if j + 1 < widths.len() && column_separators {
                    c += 1;
                }
            }
            c
        };
        let content_col_w = w.saturating_sub(cell_padding.saturating_mul(2) as usize);
        let text = cell_line_texts
            .get(cell_idx)
            .map(|s| s.as_ref())
            .unwrap_or("");
        let text_w = UnicodeWidthStr::width(text);
        let pad = content_col_w.saturating_sub(text_w);
        let align = alignments
            .get(cell_idx)
            .copied()
            .unwrap_or(crate::widgets::ColumnAlign::Left);
        let (lpad, _rpad) = match align {
            crate::widgets::ColumnAlign::Left => (0, pad),
            crate::widgets::ColumnAlign::Right => (pad, 0),
            crate::widgets::ColumnAlign::Center => (pad / 2, pad - pad / 2),
        };
        let text_start_col = cell_start_col
            .saturating_add(cell_padding as usize)
            .saturating_add(lpad);
        let text_end_col = text_start_col.saturating_add(text_w);

        let anchor_byte = if vis_col <= text_start_col {
            0
        } else if vis_col >= text_end_col {
            text.len()
        } else {
            crate::utils::text::byte_at_col(text, vis_col.saturating_sub(text_start_col))
        };

        let cell_text_start_byte = byte_cur
            .saturating_add(cell_padding as usize)
            .saturating_add(lpad);
        let cell_text_end_byte = cell_text_start_byte.saturating_add(text.len());

        Some((
            cell_idx,
            anchor_byte,
            cell_text_start_byte,
            cell_text_end_byte,
        ))
    };

    let mut col = 0usize;
    let mut byte_cursor = 0usize;
    if outer_frame {
        if visual_col == 0 {
            if !clamp {
                return None;
            }
            // Outer left border → first cell, byte 0.
            return hit_cell(0, 0, glyphs.left.len());
        }
        col = 1;
        byte_cursor = byte_cursor.saturating_add(glyphs.left.len());
    }

    // Track the byte cursor at the start of each cell for clamp fallback.
    let mut last_cell_byte_start = byte_cursor;

    for (i, &w) in widths.iter().enumerate() {
        let cell_byte_start = byte_cursor;
        let cell_start = col;
        let cell_end = col.saturating_add(w as usize);
        if visual_col >= cell_start && visual_col < cell_end {
            return hit_cell(i, visual_col, byte_cursor);
        }

        let text = cell_line_texts.get(i).map(|s| s.as_ref()).unwrap_or("");
        let content_col_w = (w as usize).saturating_sub(cell_padding.saturating_mul(2) as usize);
        let text_w = UnicodeWidthStr::width(text);
        let pad = content_col_w.saturating_sub(text_w);
        let align = alignments
            .get(i)
            .copied()
            .unwrap_or(crate::widgets::ColumnAlign::Left);
        let (lpad, rpad) = match align {
            crate::widgets::ColumnAlign::Left => (0, pad),
            crate::widgets::ColumnAlign::Right => (pad, 0),
            crate::widgets::ColumnAlign::Center => (pad / 2, pad - pad / 2),
        };
        byte_cursor = byte_cursor
            .saturating_add(cell_padding as usize)
            .saturating_add(lpad)
            .saturating_add(text.len())
            .saturating_add(rpad)
            .saturating_add(cell_padding as usize);
        last_cell_byte_start = cell_byte_start;

        col = cell_end;
        if i + 1 < widths.len() && column_separators {
            if visual_col == col {
                if !clamp {
                    return None;
                }
                // Inner border → right edge of cell i (select to text.len).
                // Pass cell_end (not cell_end-1) so vis_col >= text_end_col
                // evaluates true even with zero padding.
                return hit_cell(i, cell_end, cell_byte_start);
            }
            col = col.saturating_add(1);
            byte_cursor = byte_cursor.saturating_add(glyphs.center.len());
        }
    }

    if clamp && !widths.is_empty() {
        // Past all cells (outer right border or beyond) → last cell, text.len().
        let last = widths.len() - 1;
        return hit_cell(last, usize::MAX, last_cell_byte_start);
    }

    None
}

fn document_view_table_cell_from_coords_impl(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
    clamp_table_id: Option<usize>,
) -> Option<DocumentTableCellHit> {
    let clamp = clamp_table_id.is_some();
    if !tree.is_valid(id) {
        return None;
    }

    let node = tree.node(id);
    let NodeKind::DocumentView(doc) = &node.kind else {
        return None;
    };

    let rect = node.rect;
    let inner = rect.inner(doc.border, doc.padding);
    let cl = doc.content_layout(inner);

    if cl.content_width == 0 || cl.content_height == 0 {
        return None;
    }

    let content_x = cl.content_x;
    let content_y = cl.content_y;
    let content_w = cl.content_width;
    let content_h = cl.content_height;
    let max_x = content_x.saturating_add(content_w.saturating_sub(1) as i16);
    let max_y = content_y.saturating_add(content_h.saturating_sub(1) as i16);

    let inside = (x as i16) >= content_x
        && (x as i16) <= max_x
        && (y as i16) >= content_y
        && (y as i16) <= max_y;
    if !inside && !clamp {
        return None;
    }

    let x_clamped = clamp && ((x as i16) < content_x || (x as i16) > max_x);
    let y_clamped = clamp && ((y as i16) < content_y || (y as i16) > max_y);

    let px = if clamp {
        (x as i16).clamp(content_x, max_x)
    } else {
        x as i16
    };
    let py = if clamp {
        (y as i16).clamp(content_y, max_y)
    } else {
        y as i16
    };

    let rel_y = py.saturating_sub(content_y) as usize;
    let visual_idx = doc.scroll_offset.saturating_add(rel_y);

    // Find the visual line - when clamping, if the line at visual_idx is not a
    // matching table row, search outward for the nearest row in the target table.
    let resolve_table_line = |idx: usize| -> Option<usize> {
        let line = doc.visual_cache.lines.get(idx)?;
        if let VisualLineKind::TableRow { table_id, .. } = &line.kind
            && clamp_table_id.is_none_or(|tid| tid == *table_id)
        {
            return Some(idx);
        }
        None
    };

    let (visual_idx, y_clamped) = if let Some(idx) = resolve_table_line(visual_idx) {
        (idx, y_clamped)
    } else {
        let target_tid = clamp_table_id?;
        // Mouse is outside the table - find the nearest row from this table.
        // Depending on which direction we're out of bounds, search for the
        // closest matching row.
        let going_down = (y as i16) > py || (y as i16) >= content_y.saturating_add(rel_y as i16);
        let mut found = None;
        if going_down {
            // Dragged below: find the last row of this table at or before visual_idx,
            // or scan forward.
            for scan in (0..=visual_idx.min(doc.visual_cache.lines.len().saturating_sub(1))).rev() {
                if let Some(line) = doc.visual_cache.lines.get(scan)
                    && let VisualLineKind::TableRow { table_id, .. } = &line.kind
                    && *table_id == target_tid
                {
                    found = Some(scan);
                    break;
                }
            }
        }
        if found.is_none() {
            // Dragged above or table not found going back: scan forward from visual_idx.
            for scan in visual_idx..doc.visual_cache.lines.len() {
                if let Some(line) = doc.visual_cache.lines.get(scan)
                    && let VisualLineKind::TableRow { table_id, .. } = &line.kind
                    && *table_id == target_tid
                {
                    found = Some(scan);
                    break;
                }
            }
        }
        // We had to scan away from the cursor's visual line, so Y was effectively clamped.
        (found?, true)
    };

    let line = doc.visual_cache.lines.get(visual_idx)?;
    let VisualLineKind::TableRow {
        widths,
        alignments,
        cell_line_texts,
        table_id,
        row_index,
        row_line_index,
        cell_padding,
        border_variant,
        outer_frame,
        column_separators,
        ..
    } = &line.kind
    else {
        return None;
    };

    let rel_x = px.saturating_sub(content_x).max(0) as usize;
    let visual_col = if !doc.wrap {
        rel_x.saturating_add(doc.h_scroll_offset)
    } else {
        rel_x
    };
    let (col_index, cell_line_anchor_byte, cell_text_start_byte, cell_text_end_byte) =
        table_cell_hit_at_col(TableCellHitColParams {
            visual_col,
            widths,
            alignments,
            cell_line_texts,
            cell_padding: *cell_padding,
            border_variant: *border_variant,
            outer_frame: *outer_frame,
            column_separators: *column_separators,
            clamp,
        })?;

    Some(DocumentTableCellHit {
        table_id: *table_id,
        row_index: *row_index,
        col_index,
        row_line_index: *row_line_index,
        cell_line_anchor_byte,
        cell_text_start_byte,
        cell_text_end_byte,
        x_clamped,
        y_clamped,
    })
}

pub(crate) fn document_view_table_cell_from_coords(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
) -> Option<DocumentTableCellHit> {
    document_view_table_cell_from_coords_impl(tree, x, y, id, None)
}

pub(crate) struct TableTsvRangeParams {
    pub table_id: usize,
    pub row_start: usize,
    pub row_end: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub cursor_row_index: usize,
    pub cursor_col_index: usize,
    pub anchor_row_index: usize,
    pub anchor_col_index: usize,
    pub anchor_cell_line_anchor_byte: usize,
    pub cursor_cell_line_anchor_byte: usize,
}

pub(crate) fn table_tsv_for_range(
    doc: &crate::widgets::document_view::node::DocumentViewNode,
    range: TableTsvRangeParams,
) -> Arc<str> {
    let TableTsvRangeParams {
        table_id,
        row_start,
        row_end,
        col_start,
        col_end,
        cursor_row_index,
        cursor_col_index,
        anchor_row_index,
        anchor_col_index,
        anchor_cell_line_anchor_byte,
        cursor_cell_line_anchor_byte,
    } = range;
    // For the drag endpoint cell (when it is not the anchor cell), choose prefix vs suffix
    // using column position relative to the anchor - not overall reading order. Columns
    // strictly left of the anchor extend selection rightward through the cell (suffix from
    // the pointer); same column or any column to the right use the standard "from left" cut
    // (prefix up to the pointer). See `reverse_table_rect_copy_*` tests.
    let cursor_cell_use_suffix = cursor_col_index < anchor_col_index;
    let mut first_lines: BTreeMap<usize, Vec<Arc<str>>> = BTreeMap::new();
    for line in &doc.visual_cache.lines {
        if let VisualLineKind::TableRow {
            table_id: tid,
            row_index,
            row_line_index,
            full_cell_texts,
            ..
        } = &line.kind
            && *tid == table_id
            && *row_line_index == 0
        {
            first_lines
                .entry(*row_index)
                .or_insert_with(|| full_cell_texts.clone());
        }
    }

    let mut lines = Vec::new();
    for row in row_start..=row_end {
        let mut cols = Vec::new();
        for col in col_start..=col_end {
            let full_text = first_lines
                .get(&row)
                .and_then(|cells| cells.get(col))
                .map(|s| s.as_ref())
                .unwrap_or("");
            if row == cursor_row_index && col == cursor_col_index {
                if row == anchor_row_index && col == anchor_col_index {
                    if anchor_cell_line_anchor_byte == usize::MAX
                        && cursor_cell_line_anchor_byte == usize::MAX
                    {
                        cols.push(full_text.to_string());
                    } else {
                        let mut start = anchor_cell_line_anchor_byte.min(full_text.len());
                        while start > 0 && !full_text.is_char_boundary(start) {
                            start = start.saturating_sub(1);
                        }
                        let mut end = cursor_cell_line_anchor_byte.min(full_text.len());
                        while end > 0 && !full_text.is_char_boundary(end) {
                            end = end.saturating_sub(1);
                        }
                        let (start, end) = if start <= end {
                            (start, end)
                        } else {
                            (end, start)
                        };
                        cols.push(full_text[start..end].to_string());
                    }
                } else if cursor_cell_line_anchor_byte == usize::MAX {
                    // Pointer left the table (clamped coords): full cell, including suffix-mode.
                    cols.push(full_text.to_string());
                } else {
                    let mut cursor = cursor_cell_line_anchor_byte.min(full_text.len());
                    while cursor > 0 && !full_text.is_char_boundary(cursor) {
                        cursor = cursor.saturating_sub(1);
                    }
                    if cursor_cell_use_suffix {
                        cols.push(full_text[cursor..].to_string());
                    } else {
                        cols.push(full_text[..cursor].to_string());
                    }
                }
            } else {
                cols.push(full_text.to_string());
            }
        }
        lines.push(cols.join("\t"));
    }

    Arc::from(lines.join("\n"))
}

struct DocumentViewTableRectSelectionParams {
    x: u16,
    y: u16,
    id: NodeId,
    anchor_table_id: usize,
    anchor_row: usize,
    anchor_col: usize,
    anchor_row_line_index: usize,
    anchor_cell_line_anchor_byte: usize,
}

fn document_view_table_rect_selection_from_coords(
    tree: &NodeTree,
    params: DocumentViewTableRectSelectionParams,
) -> Option<DocumentTableRectSelection> {
    let DocumentViewTableRectSelectionParams {
        x,
        y,
        id,
        anchor_table_id,
        anchor_row,
        anchor_col,
        anchor_row_line_index,
        anchor_cell_line_anchor_byte,
    } = params;
    let hit = document_view_table_cell_from_coords_impl(tree, x, y, id, Some(anchor_table_id))?;
    if hit.table_id != anchor_table_id {
        return None;
    }

    let row_start = anchor_row.min(hit.row_index);
    let row_end = anchor_row.max(hit.row_index);
    let col_start = anchor_col.min(hit.col_index);
    let col_end = anchor_col.max(hit.col_index);

    let node = tree.node(id);
    let NodeKind::DocumentView(doc) = &node.kind else {
        return None;
    };

    // When the mouse is outside the table bounds, select full cells
    // (use usize::MAX so table_tsv_for_range captures entire cell text).
    let outside = hit.x_clamped || hit.y_clamped;
    let effective_anchor_byte = if outside {
        usize::MAX
    } else {
        anchor_cell_line_anchor_byte
    };
    let effective_cursor_byte = if outside {
        usize::MAX
    } else {
        hit.cell_line_anchor_byte
    };

    let tsv_text = table_tsv_for_range(
        doc,
        TableTsvRangeParams {
            table_id: anchor_table_id,
            row_start,
            row_end,
            col_start,
            col_end,
            cursor_row_index: hit.row_index,
            cursor_col_index: hit.col_index,
            anchor_row_index: anchor_row,
            anchor_col_index: anchor_col,
            anchor_cell_line_anchor_byte: effective_anchor_byte,
            cursor_cell_line_anchor_byte: effective_cursor_byte,
        },
    );

    Some(DocumentTableRectSelection {
        table_id: anchor_table_id,
        row_start,
        row_end,
        col_start,
        col_end,
        anchor_row_index: anchor_row,
        anchor_col_index: anchor_col,
        anchor_row_line_index,
        anchor_cell_line_anchor_byte: effective_anchor_byte,
        cursor_row_index: hit.row_index,
        cursor_col_index: hit.col_index,
        cursor_row_line_index: hit.row_line_index,
        cursor_cell_line_anchor_byte: effective_cursor_byte,
        tsv_text,
    })
}

/// Handle document view mouse drag selection.
#[allow(clippy::type_complexity)]
pub(crate) fn handle_document_view_drag(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
    anchor: DocumentViewDragAnchor,
) -> Option<(
    usize,
    Option<usize>,
    Option<DocumentTableRectSelection>,
    Option<Callback<DocumentSelectEvent>>,
    Option<Arc<str>>,
)> {
    if !tree.is_valid(id) {
        return None;
    }

    let node = tree.node(id);
    let NodeKind::DocumentView(doc) = &node.kind else {
        return None;
    };

    match anchor {
        DocumentViewDragAnchor::Linear(anchor) => {
            let cursor = document_view_cursor_from_coords(tree, x, y, id)?;
            let clamped_cursor = cursor.min(doc.visual_cache.flat_text.len());
            let clamped_anchor = anchor.min(doc.visual_cache.flat_text.len());
            let start = clamped_anchor.min(clamped_cursor);
            let end = clamped_anchor.max(clamped_cursor);
            let on_select = doc.on_select.clone();
            let selected = on_select.as_ref().map(|_| {
                document_view_selected_text_from_node(doc, start, end, false)
                    .map(Arc::<str>::from)
                    .unwrap_or_else(|| Arc::from(""))
            });

            Some((
                clamped_cursor,
                Some(clamped_anchor),
                None,
                on_select,
                selected,
            ))
        }
        DocumentViewDragAnchor::TableCell {
            table_id,
            row_index,
            col_index,
            row_line_index,
            cell_line_anchor_byte,
        } => {
            let selection = document_view_table_rect_selection_from_coords(
                tree,
                DocumentViewTableRectSelectionParams {
                    x,
                    y,
                    id,
                    anchor_table_id: table_id,
                    anchor_row: row_index,
                    anchor_col: col_index,
                    anchor_row_line_index: row_line_index,
                    anchor_cell_line_anchor_byte: cell_line_anchor_byte,
                },
            )?;
            let on_select = doc.on_select.clone();
            let selected = on_select.as_ref().map(|_| selection.tsv_text.clone());
            Some((
                doc.selection_cursor,
                doc.selection_anchor,
                Some(selection),
                on_select,
                selected,
            ))
        }
    }
}
