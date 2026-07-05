use crate::widgets::table::{resolved_row_total_height, table_header_reserved_height};
use crate::widgets::{ColumnWidth, Table};

pub(crate) fn measure_table(table: &Table) -> (u16, u16) {
    // Estimate width from constraints
    let mut min_w = 0u16;
    for width in &table.widths {
        match width {
            ColumnWidth::Fixed(w) | ColumnWidth::Min(w) => {
                min_w = min_w.saturating_add(*w);
            }
            _ => {}
        }
    }
    // Add spacing
    if !table.widths.is_empty() {
        let spacing = table
            .column_spacing
            .saturating_mul(table.widths.len() as u16 - 1);
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

        assert_eq!(measure_table(&table), (0, 9));
    }

    #[test]
    fn row_gap_contributes_between_header_and_rows_only() {
        let table = Table::new()
            .header(TableRow::new(["header"]))
            .rows([TableRow::new(["row-1"]), TableRow::new(["row-2"])])
            .row_gap(2);

        assert_eq!(measure_table(&table), (0, 7));
    }

    #[test]
    fn row_gap_not_added_after_header_only_table() {
        let table = Table::new().header(TableRow::new(["header"])).row_gap(2);

        assert_eq!(measure_table(&table), (0, 1));
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
        assert_eq!(measure_table(&table), (0, 8));
    }

    #[test]
    fn row_gap_does_not_add_height_without_data_rows() {
        let table = Table::new()
            .row_gap(2)
            .header(TableRow::new(["header"]).height(1));

        assert_eq!(measure_table(&table), (0, 1));
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
