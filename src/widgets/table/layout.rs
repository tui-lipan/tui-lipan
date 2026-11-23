use std::borrow::Cow;

use unicode_width::UnicodeWidthStr;

use crate::widgets::table::{resolved_row_total_height, table_header_reserved_height};
use crate::widgets::{ColumnWidth, Table, TableDisclosureState, TableRow, TableRowRole};

fn cell_display_width(content: &str) -> u16 {
    content
        .lines()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .min(u16::MAX as usize) as u16
}

pub(crate) fn table_inspector_prefix_width(
    row: &TableRow,
    inspector: bool,
    indent_size: u16,
    collapsed_symbol: &str,
    expanded_symbol: &str,
) -> u16 {
    if !inspector {
        return 0;
    }

    let mut width = row.depth.saturating_mul(indent_size);
    if let Some(disclosure) = row.disclosure {
        let symbol = match disclosure {
            TableDisclosureState::Collapsed => collapsed_symbol,
            TableDisclosureState::Expanded => expanded_symbol,
        };
        if !symbol.is_empty() {
            width = width
                .saturating_add(UnicodeWidthStr::width(symbol).min(u16::MAX as usize) as u16)
                .saturating_add(1);
        }
    }
    width
}

pub(crate) fn resolved_column_widths<'a>(
    header: Option<&TableRow>,
    rows: &[TableRow],
    widths: &'a [ColumnWidth],
    selection_symbol: Option<&str>,
    unselected_symbol: Option<&str>,
    inspector_prefix_width: impl Fn(&TableRow) -> u16,
) -> Cow<'a, [ColumnWidth]> {
    let content_cols = header
        .map(|h| h.cells.len())
        .into_iter()
        .chain(
            rows.iter()
                .filter(|r| matches!(r.role, TableRowRole::Normal))
                .map(|r| r.cells.len()),
        )
        .max()
        .unwrap_or(0);

    if widths.len() >= content_cols {
        return Cow::Borrowed(widths);
    }

    let unselected_w = unselected_symbol
        .map(|s| UnicodeWidthStr::width(s).min(u16::MAX as usize) as u16)
        .unwrap_or(0);
    let symbol_w = selection_symbol
        .map(|s| UnicodeWidthStr::width(s).min(u16::MAX as usize) as u16)
        .unwrap_or(0)
        .max(unselected_w);

    let start_col = widths.len();
    let missing_cols = content_cols - start_col;
    let mut derived = vec![0u16; missing_cols];

    if let Some(h) = header {
        for (col_idx, cell) in h.cells.iter().enumerate().skip(start_col) {
            let mut w = cell_display_width(cell.content.as_ref());
            if col_idx == 0 {
                w = w.saturating_add(unselected_w);
            }
            let slot = &mut derived[col_idx - start_col];
            *slot = (*slot).max(w);
        }
    }

    for row in rows
        .iter()
        .filter(|r| matches!(r.role, TableRowRole::Normal))
    {
        for (col_idx, cell) in row.cells.iter().enumerate().skip(start_col) {
            let mut w = cell_display_width(cell.content.as_ref());
            if col_idx == 0 {
                w = w
                    .saturating_add(symbol_w)
                    .saturating_add(inspector_prefix_width(row));
            }
            let slot = &mut derived[col_idx - start_col];
            *slot = (*slot).max(w);
        }
    }

    let mut out = Vec::with_capacity(content_cols);
    out.extend_from_slice(widths);
    out.extend(derived.into_iter().map(|w| ColumnWidth::Min(w.max(1))));
    Cow::Owned(out)
}

pub(crate) fn resolved_table_widths(table: &Table) -> Cow<'_, [ColumnWidth]> {
    resolved_column_widths(
        table.header.as_ref(),
        &table.rows,
        &table.widths,
        table.selection_symbol.as_deref(),
        table.unselected_symbol.as_deref(),
        |row| {
            table_inspector_prefix_width(
                row,
                table.inspector,
                table.inspector_indent_size,
                table.inspector_collapsed_symbol.as_ref(),
                table.inspector_expanded_symbol.as_ref(),
            )
        },
    )
}

pub(crate) fn measure_table(table: &Table) -> (u16, u16) {
    let widths = resolved_table_widths(table);

    // Estimate width from constraints
    let mut min_w = 0u16;
    for width in widths.iter() {
        match width {
            ColumnWidth::Fixed(w) | ColumnWidth::Min(w) => {
                min_w = min_w.saturating_add(*w);
            }
            _ => {}
        }
    }
    // Add spacing
    if !widths.is_empty() {
        let spacing = table.column_spacing.saturating_mul(widths.len() as u16 - 1);
        min_w = min_w.saturating_add(spacing);
    }

    // Estimate height
    let mut h = 0usize;
    h += table_header_reserved_height(table.header.as_ref(), table.rows.len(), table.row_gap)
        as usize;
    for (idx, row) in table.rows.iter().enumerate() {
        if idx > 0 {
            h += table.row_gap as usize;
        }
        h += resolved_row_total_height(row) as usize;
    }

    let mut w = min_w;

    w = w.saturating_add(table.padding.horizontal());
    let h = h.saturating_add(table.padding.vertical() as usize);

    let (mut w, mut h) = (w, h as u16);

    if table.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    (w, h)
}

#[cfg(test)]
mod tests {
    use super::measure_table;
    use crate::widgets::table::{row_index_at_visual_offset, visible_rows_for_height};
    use crate::widgets::{ColumnWidth, Table, TableRow};

    #[test]
    fn fixed_column_width_summation() {
        let table = Table::new()
            .widths([
                ColumnWidth::Fixed(4),
                ColumnWidth::Fixed(6),
                ColumnWidth::Fixed(2),
            ])
            .column_spacing(0);

        assert_eq!(measure_table(&table), (12, 0));
    }

    #[test]
    fn spacing_between_columns() {
        let table = Table::new()
            .widths([
                ColumnWidth::Fixed(3),
                ColumnWidth::Min(2),
                ColumnWidth::Fixed(5),
            ])
            .column_spacing(2);

        assert_eq!(measure_table(&table), (14, 0));
    }

    #[test]
    fn header_rows_height_contribution() {
        let table = Table::new()
            .header(TableRow::new(["header"]).height(2).bottom_margin(1))
            .rows([
                TableRow::new(["row-1"]).height(3).bottom_margin(2),
                TableRow::new(["row-2"]).height(1),
            ]);

        assert_eq!(measure_table(&table), (6, 9));
    }

    #[test]
    fn row_gap_contributes_between_header_and_rows_only() {
        let table = Table::new()
            .header(TableRow::new(["header"]))
            .rows([TableRow::new(["row-1"]), TableRow::new(["row-2"])])
            .row_gap(2);

        assert_eq!(measure_table(&table), (6, 7));
    }

    #[test]
    fn row_gap_not_added_after_header_only_table() {
        let table = Table::new().header(TableRow::new(["header"])).row_gap(2);

        assert_eq!(measure_table(&table), (6, 1));
    }

    #[test]
    fn border_padding_impact_on_measured_size() {
        let table = Table::new()
            .widths([ColumnWidth::Fixed(5)])
            .rows([TableRow::new(["row"])])
            .padding((3, 2))
            .border(true);

        assert_eq!(measure_table(&table), (11, 9));
    }

    #[test]
    fn empty_table_edge_case() {
        let table = Table::new();

        assert_eq!(measure_table(&table), (0, 0));
    }

    #[test]
    fn row_gap_contributes_to_measured_height_between_rendered_rows() {
        let table = Table::new()
            .row_gap(1)
            .header(TableRow::new(["header"]).height(1).bottom_margin(1))
            .rows([
                TableRow::new(["row-1"]).height(2).bottom_margin(1),
                TableRow::new(["row-2"]).height(1),
            ]);

        // header height + header bottom margin + header/data gap
        // + first row height + first row bottom margin + data/data gap
        // + final row height; no row_gap after the final row.
        assert_eq!(measure_table(&table), (6, 8));
    }

    #[test]
    fn row_gap_does_not_add_height_without_data_rows() {
        let table = Table::new()
            .row_gap(2)
            .header(TableRow::new(["header"]).height(1));

        assert_eq!(measure_table(&table), (6, 1));
    }

    #[test]
    fn default_widths_derive_min_from_widest_cell_and_symbol() {
        let table = Table::new()
            .header(TableRow::new(["PID", "Name", "CPU"]))
            .rows([
                TableRow::new(["1204", "lipan", "2.1"]),
                TableRow::new(["2048", "cargo", "18.4"]),
            ])
            .selection_symbol(Some("> "))
            .unselected_symbol(Some("  "));

        assert_eq!(
            super::resolved_table_widths(&table).as_ref(),
            &[
                ColumnWidth::Min(6),
                ColumnWidth::Min(5),
                ColumnWidth::Min(4),
            ]
        );
        // 6 + 5 + 4 + 2*column_spacing(1) = 17
        assert_eq!(measure_table(&table), (17, 3));
    }

    #[test]
    fn partial_widths_preserve_explicit_and_derive_remaining_columns() {
        let table = Table::new()
            .header(TableRow::new(["PID", "Name", "CPU"]))
            .rows([TableRow::new(["1204", "lipan", "18.4"])])
            .widths([ColumnWidth::Fixed(8)]);

        assert_eq!(
            super::resolved_table_widths(&table).as_ref(),
            &[
                ColumnWidth::Fixed(8),
                ColumnWidth::Min(5),
                ColumnWidth::Min(4),
            ]
        );
    }

    #[test]
    fn empty_column_cells_floor_default_width_at_one() {
        let table = Table::new().rows([TableRow::new(["", "ok"])]);

        assert_eq!(
            super::resolved_table_widths(&table).as_ref(),
            &[ColumnWidth::Min(1), ColumnWidth::Min(2)]
        );
    }

    #[test]
    fn visible_rows_for_height_accounts_for_row_gap_between_visible_rows() {
        let rows = vec![
            TableRow::new(["row-1"]),
            TableRow::new(["row-2"]),
            TableRow::new(["row-3"]),
        ];

        assert_eq!(visible_rows_for_height(&rows, 0, 1, 1), 1);
        assert_eq!(visible_rows_for_height(&rows, 0, 3, 1), 2);
        assert_eq!(visible_rows_for_height(&rows, 0, 4, 1), 2);
        assert_eq!(visible_rows_for_height(&rows, 0, 5, 1), 3);
        assert_eq!(visible_rows_for_height(&rows, 1, 3, 1), 2);
    }

    #[test]
    fn visible_rows_for_height_combines_row_gap_with_bottom_margin() {
        let rows = vec![
            TableRow::new(["row-1"]).bottom_margin(1),
            TableRow::new(["row-2"]),
        ];

        assert_eq!(visible_rows_for_height(&rows, 0, 3, 1), 1);
        assert_eq!(visible_rows_for_height(&rows, 0, 4, 1), 2);
    }

    #[test]
    fn row_index_at_visual_offset_returns_none_for_gap_rows() {
        let rows = vec![
            TableRow::new(["row-1"]),
            TableRow::new(["row-2"]),
            TableRow::new(["row-3"]),
        ];

        assert_eq!(row_index_at_visual_offset(&rows, 0, 0, 1), Some(0));
        assert_eq!(row_index_at_visual_offset(&rows, 0, 1, 1), None);
        assert_eq!(row_index_at_visual_offset(&rows, 0, 2, 1), Some(1));
        assert_eq!(row_index_at_visual_offset(&rows, 0, 3, 1), None);
        assert_eq!(row_index_at_visual_offset(&rows, 0, 4, 1), Some(2));
    }
}
