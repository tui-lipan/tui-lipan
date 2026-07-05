use super::Heatmap;
use unicode_width::UnicodeWidthStr;

/// Compute the minimum intrinsic size for a heatmap.
pub fn measure_heatmap(heatmap: &Heatmap) -> (u16, u16) {
    let rows = heatmap.data.len();
    let cols = heatmap.data.iter().map(|row| row.len()).max().unwrap_or(0);

    if rows == 0 || cols == 0 {
        return (0, 0);
    }

    let cell_w = heatmap.effective_cell_width();

    // Row label gutter: width of longest label + 1 space separator.
    let label_gutter: u16 = if heatmap.row_labels.is_empty() {
        0
    } else {
        let max_label = heatmap
            .row_labels
            .iter()
            .map(|l| UnicodeWidthStr::width(l.as_ref()).min(u16::MAX as usize) as u16)
            .max()
            .unwrap_or(0);
        max_label.saturating_add(1)
    };

    // Column header: 1 row if any column labels exist.
    let header_rows: u16 = if heatmap.column_labels.is_empty() {
        0
    } else {
        1
    };

    // Legend: 1 row if enabled, plus optional spacing above it.
    let legend_rows: u16 = if heatmap.show_legend {
        1u16.saturating_add(heatmap.legend_spacing)
    } else {
        0
    };

    let data_w = (cols as u16)
        .saturating_mul(cell_w)
        .saturating_add((cols.saturating_sub(1) as u16).saturating_mul(heatmap.gap_x));
    let data_h =
        (rows as u16).saturating_add((rows.saturating_sub(1) as u16).saturating_mul(heatmap.gap_y));

    let mut w = label_gutter.saturating_add(data_w);
    let mut h = header_rows
        .saturating_add(data_h)
        .saturating_add(legend_rows);

    w = w.saturating_add(heatmap.padding.horizontal());
    h = h.saturating_add(heatmap.padding.vertical());

    if heatmap.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    (w, h)
}
