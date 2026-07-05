use crate::core::element::ElementKind;
use crate::layout::measure::min_size_constrained;
use crate::style::{Align, Rect};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlowRow {
    pub y: i16,
    // Reserved for future row-level justify logic. Today it's mainly useful
    // during measurement and debugging.
    pub width: u16,
    pub height: u16,
    pub items: Vec<(usize, Rect)>,
}

/// Compute the chrome (border + padding) size for a Flow.
fn flow_chrome(flow: &super::Flow) -> (u16, u16) {
    let chrome_w = flow.padding.horizontal() + if flow.border { 2 } else { 0 };
    let chrome_h = flow.padding.vertical() + if flow.border { 2 } else { 0 };
    (chrome_w, chrome_h)
}

pub(crate) fn pack_rows(flow: &super::Flow, bounds: Rect) -> Vec<FlowRow> {
    let inner = bounds.inner(flow.border, flow.padding);

    let mut measured = Vec::new();
    for (idx, child) in flow.children.iter().enumerate() {
        if matches!(child.kind, ElementKind::Portal(_)) {
            continue;
        }
        let (cw, ch) = min_size_constrained(child, Some(inner.w), None);
        measured.push((idx, cw, ch));
    }

    let row_gap = flow.row_gap.unwrap_or(flow.gap);
    pack_rows_from_sizes(
        &measured,
        FlowPackParams {
            available_w: inner.w,
            gap: flow.gap,
            row_gap,
            align: flow.align,
            origin_x: inner.x,
            origin_y: inner.y,
        },
    )
}

struct FlowPackParams {
    available_w: u16,
    gap: u16,
    row_gap: u16,
    align: Align,
    origin_x: i16,
    origin_y: i16,
}

fn cross_offset(align: Align, row_h: u16, child_h: u16) -> u16 {
    match align {
        Align::Start | Align::Stretch => 0,
        Align::Center => row_h.saturating_sub(child_h) / 2,
        Align::End => row_h.saturating_sub(child_h),
    }
}

fn pack_rows_from_sizes(measured: &[(usize, u16, u16)], params: FlowPackParams) -> Vec<FlowRow> {
    let FlowPackParams {
        available_w,
        gap,
        row_gap,
        align,
        origin_x,
        origin_y,
    } = params;
    if measured.is_empty() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    let mut row_items: Vec<(usize, u16, u16)> = Vec::new();
    let mut row_w = 0u16;
    let mut row_h = 0u16;
    let mut row_y = origin_y;

    let flush_row = |rows: &mut Vec<FlowRow>,
                     row_items: &mut Vec<(usize, u16, u16)>,
                     row_w: &mut u16,
                     row_h: &mut u16,
                     row_y: i16| {
        if row_items.is_empty() {
            return;
        }

        // We intentionally recompute x positions from 0 on flush. It keeps the
        // accumulation path simple and is negligible for typical chip/badge
        // counts.
        let mut x_cursor = 0u16;
        let mut items = Vec::with_capacity(row_items.len());
        for (item_idx, (child_idx, child_w, child_h)) in row_items.iter().enumerate() {
            let child_x = origin_x.saturating_add(x_cursor as i16);
            let child_y = row_y.saturating_add(cross_offset(align, *row_h, *child_h) as i16);
            items.push((
                *child_idx,
                Rect {
                    x: child_x,
                    y: child_y,
                    w: *child_w,
                    h: *child_h,
                },
            ));

            if item_idx + 1 < row_items.len() {
                x_cursor = x_cursor.saturating_add(*child_w).saturating_add(gap);
            }
        }

        rows.push(FlowRow {
            y: row_y,
            width: *row_w,
            height: *row_h,
            items,
        });

        row_items.clear();
        *row_w = 0;
        *row_h = 0;
    };

    for (idx, measured_w, measured_h) in measured.iter().copied() {
        let child_w = measured_w.min(available_w);
        let child_h = measured_h;

        if row_items.is_empty() {
            row_items.push((idx, child_w, child_h));
            row_w = child_w;
            row_h = child_h;
            continue;
        }

        let next_w = row_w.saturating_add(gap).saturating_add(child_w);
        if next_w <= available_w {
            row_items.push((idx, child_w, child_h));
            row_w = next_w;
            row_h = row_h.max(child_h);
            continue;
        }

        let finished_row_h = row_h;
        flush_row(&mut rows, &mut row_items, &mut row_w, &mut row_h, row_y);
        row_y = row_y
            .saturating_add(finished_row_h as i16)
            .saturating_add(row_gap as i16);

        row_items.push((idx, child_w, child_h));
        row_w = child_w;
        row_h = child_h;
    }

    flush_row(&mut rows, &mut row_items, &mut row_w, &mut row_h, row_y);
    rows
}

pub(crate) fn measure_flow(
    flow: &super::Flow,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let (chrome_w, chrome_h) = flow_chrome(flow);

    let Some(max_w) = max_w else {
        // With no width budget the natural width is the single-row layout: every
        // child laid out left-to-right with no wrapping (sum of widths + gaps),
        // and the tallest child as the height. This matches how `HStack` reports
        // its intrinsic width, so an `Auto`-width Flow measures correctly when it
        // is a main-axis child of another stack (e.g. inside a SpaceBetween
        // HStack) instead of collapsing to its widest single child. Shrinking is
        // still possible because the stack shrink floor for Auto children is one
        // cell, not this measured base.
        let mut w = 0u16;
        let mut h = 0u16;
        let mut visible = 0u16;
        for child in &flow.children {
            if matches!(child.kind, ElementKind::Portal(_)) {
                continue;
            }
            let (cw, ch) = min_size_constrained(child, None, max_h);
            if cw > 0 {
                if visible > 0 {
                    w = w.saturating_add(flow.gap);
                }
                w = w.saturating_add(cw);
                visible = visible.saturating_add(1);
            }
            h = h.max(ch);
        }
        return (w.saturating_add(chrome_w), h.saturating_add(chrome_h));
    };

    let inner_w = max_w.saturating_sub(chrome_w);
    let rows = pack_rows(
        flow,
        Rect {
            x: 0,
            y: 0,
            w: inner_w,
            h: 0,
        },
    );

    let mut measured_w = 0u16;
    let mut measured_h = 0u16;
    for row in &rows {
        measured_w = measured_w.max(row.width);
        measured_h = measured_h.saturating_add(row.height);
    }
    if rows.len() > 1 {
        let row_gap = flow.row_gap.unwrap_or(flow.gap);
        measured_h = measured_h.saturating_add(row_gap.saturating_mul(rows.len() as u16 - 1));
    }

    (
        measured_w.saturating_add(chrome_w),
        measured_h.saturating_add(chrome_h),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Align;

    #[test]
    fn pack_rows_keeps_items_on_one_row_when_they_fit() {
        let rows = pack_rows_from_sizes(
            &[(0, 10, 1), (1, 10, 1), (2, 10, 1)],
            FlowPackParams {
                available_w: 35,
                gap: 0,
                row_gap: 0,
                align: Align::Start,
                origin_x: 0,
                origin_y: 0,
            },
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].items.len(), 3);
    }

    #[test]
    fn pack_rows_wraps_when_next_item_overflows() {
        let rows = pack_rows_from_sizes(
            &[(0, 10, 1), (1, 10, 1), (2, 10, 1)],
            FlowPackParams {
                available_w: 30,
                gap: 2,
                row_gap: 2,
                align: Align::Start,
                origin_x: 0,
                origin_y: 0,
            },
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].items.len(), 2);
        assert_eq!(rows[1].items.len(), 1);
    }

    #[test]
    fn oversized_single_item_is_clamped_to_row_width() {
        let rows = pack_rows_from_sizes(
            &[(0, 20, 1)],
            FlowPackParams {
                available_w: 8,
                gap: 1,
                row_gap: 1,
                align: Align::Start,
                origin_x: 0,
                origin_y: 0,
            },
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].items[0].1.w, 8);
    }

    #[test]
    fn row_gap_contributes_to_total_height() {
        let rows = pack_rows_from_sizes(
            &[(0, 10, 1), (1, 10, 2), (2, 10, 3)],
            FlowPackParams {
                available_w: 12,
                gap: 2,
                row_gap: 2,
                align: Align::Start,
                origin_x: 0,
                origin_y: 0,
            },
        );

        let mut total_h = rows
            .iter()
            .fold(0u16, |acc, row| acc.saturating_add(row.height));
        total_h = total_h.saturating_add(2u16.saturating_mul(rows.len() as u16 - 1));

        assert_eq!(total_h, 10);
    }

    #[test]
    fn row_gap_is_independent_of_item_gap() {
        // Three items, one per row (each as wide as the row), with a large
        // horizontal gap but zero row gap: rows must stack with no vertical
        // spacing between them.
        let rows = pack_rows_from_sizes(
            &[(0, 10, 1), (1, 10, 1), (2, 10, 1)],
            FlowPackParams {
                available_w: 10,
                gap: 4,
                row_gap: 0,
                align: Align::Start,
                origin_x: 0,
                origin_y: 0,
            },
        );

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].y, 0);
        assert_eq!(rows[1].y, 1);
        assert_eq!(rows[2].y, 2);
    }

    #[test]
    fn center_alignment_offsets_shorter_children_within_row() {
        let rows = pack_rows_from_sizes(
            &[(0, 5, 1), (1, 5, 3)],
            FlowPackParams {
                available_w: 20,
                gap: 1,
                row_gap: 1,
                align: Align::Center,
                origin_x: 0,
                origin_y: 0,
            },
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].height, 3);
        assert_eq!(rows[0].items[0].1.y, 1);
        assert_eq!(rows[0].items[1].1.y, 0);
    }

    #[test]
    fn pack_rows_offsets_by_inner_origin() {
        // When padding is (1, 2) (top=1, right=2, bottom=1, left=2), inner
        // origin shifts to (2, 1). Children should start at that offset.
        let rows = pack_rows_from_sizes(
            &[(0, 10, 1)],
            FlowPackParams {
                available_w: 30,
                gap: 0,
                row_gap: 0,
                align: Align::Start,
                origin_x: 2,
                origin_y: 1,
            },
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].items[0].1.x, 2);
        assert_eq!(rows[0].items[0].1.y, 1);
    }
}
