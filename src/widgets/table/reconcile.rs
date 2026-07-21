use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{Length, Rect};
use crate::widgets::Table;

use super::layout::measure_table;

pub(crate) fn reconcile_table(
    tree: &mut NodeTree,
    id: NodeId,
    table: &Table,
    rect: Rect,
) -> NodeId {
    let (w, h) = measure_table(table);

    let mut rect = rect;
    if matches!(table.width, Length::Auto) {
        rect.w = w.min(rect.w);
    }
    if matches!(table.height, Length::Auto) {
        rect.h = h.min(rect.h);
    }

    {
        let node = tree.node_mut(id);

        let (old_offset, scroll_override) = if let NodeKind::Table(node) = &node.kind {
            (node.offset, node.scroll_override)
        } else {
            (0, None)
        };

        let inner = rect.inner(table.border, table.padding);
        let len = table.rows.len();
        let max_display = inner.h as usize;

        let header_h = crate::widgets::table::table_header_reserved_height(
            table.header.as_ref(),
            table.rows.len(),
            table.row_gap,
        );
        let available_h = max_display.saturating_sub(header_h as usize) as u16;
        let offset_for_measure = scroll_override.unwrap_or(old_offset);
        let max_display_rows = crate::widgets::table::visible_rows_for_height(
            &table.rows,
            offset_for_measure,
            available_h,
            table.row_gap,
        );

        let (new_offset, top_indicator, bottom_indicator, bottom_count) = if max_display_rows == 0
            || len == 0
        {
            (0, false, false, 0)
        } else if let Some(forced) = scroll_override {
            let (start, end, top, bot) =
                crate::widgets::list::utils::calc_list_window(forced, len, max_display_rows);
            (start, top, bot, len.saturating_sub(end))
        } else if let Some(selected) = table.selected {
            if table.show_scroll_indicators {
                let smart_off = crate::widgets::scroll::smart_list_offset_with_indicators(
                    old_offset,
                    selected,
                    len,
                    max_display_rows,
                );
                let (start, end, top, bot) =
                    crate::widgets::list::utils::calc_list_window(smart_off, len, max_display_rows);
                (start, top, bot, len.saturating_sub(end))
            } else {
                (
                    crate::widgets::scroll::smart_list_offset(
                        old_offset,
                        selected,
                        len,
                        max_display_rows as u16,
                    ),
                    false,
                    false,
                    0,
                )
            }
        } else if table.show_scroll_indicators {
            // No selection: keep the previous offset, but still derive the
            // overflow indicators from it so a read-only table keeps its
            // "N more" affordance.
            let (start, end, top, bot) =
                crate::widgets::list::utils::calc_list_window(old_offset, len, max_display_rows);
            (start, top, bot, len.saturating_sub(end))
        } else {
            // No selection: keep the previous offset (clamped below).
            (old_offset, false, false, 0)
        };

        let new_offset = new_offset.min(len.saturating_sub(1));

        let mut next_scroll_override = None;
        if scroll_override.is_some() {
            next_scroll_override = Some(new_offset);
        }

        let is_standalone = table.scrollbar
            && (!table.border
                || matches!(
                    table.scrollbar_config.variant,
                    crate::style::ScrollbarVariant::Standalone
                ));

        // Determine if we actually need a scrollbar (adaptive).
        let scrollable = len > max_display_rows;
        let actual_scrollbar = if is_standalone {
            scrollable
        } else {
            table.scrollbar
        };

        node.rect = rect;
        node.children.clear();
        node.kind = NodeKind::from(table.clone());

        if let NodeKind::Table(node) = &mut node.kind {
            node.offset = new_offset;
            node.top_indicator = top_indicator;
            node.bottom_indicator = bottom_indicator;
            node.bottom_count = bottom_count;
            node.scroll_override = next_scroll_override;
            node.scrollbar = actual_scrollbar;
        }
    }

    tree.register_scrollbar_zone(id);

    id
}

#[cfg(test)]
mod tests {
    use super::reconcile_table;
    use crate::core::node::{NodeKind, NodeTree};
    use crate::style::Rect;
    use crate::widgets::{Table, TableRow};

    fn overflow_indicators(selected: Option<usize>) -> (bool, bool, usize) {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let table = Table::new()
            .rows((0..30).map(|i| TableRow::new([format!("row {i}")])))
            .selected(selected)
            .show_scroll_indicators(true);

        reconcile_table(
            &mut tree,
            id,
            &table,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 10,
            },
        );

        let NodeKind::Table(node) = &tree.node(id).kind else {
            panic!("expected table node");
        };
        (node.top_indicator, node.bottom_indicator, node.bottom_count)
    }

    #[test]
    fn selected_none_still_reports_overflow_indicators() {
        // A read-only table must keep its "N more below" affordance; the
        // no-selection scroll branch used to hardcode the indicators off.
        let (top, bottom, bottom_count) = overflow_indicators(None);
        assert!(!top);
        assert!(bottom, "overflowing table should report a bottom indicator");
        assert!(bottom_count > 0, "bottom_count = {bottom_count}");

        // Matches what an equivalent selected table reports from the same offset.
        assert_eq!(overflow_indicators(Some(0)), (top, bottom, bottom_count));
    }
}
