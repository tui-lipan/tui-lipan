use std::sync::Arc;

use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{Length, Rect};
use crate::widgets::{Table, TableRow};

use super::layout::{derived_table_widths, measure_table};
use super::node::TableNode;

fn headers_have_same_cell_text(a: Option<&TableRow>, b: Option<&TableRow>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.cells.len() == b.cells.len()
                && a.cells
                    .iter()
                    .zip(b.cells.iter())
                    .all(|(x, y)| x.content == y.content)
        }
        _ => false,
    }
}

fn can_reuse_resolved_widths(old: &TableNode, table: &Table) -> bool {
    Arc::ptr_eq(&old.rows, &table.rows)
        && old.widths == table.widths
        && old.selection_symbol == table.selection_symbol
        && old.unselected_symbol == table.unselected_symbol
        && old.inspector == table.inspector
        && old.inspector_indent_size == table.inspector_indent_size
        && old.inspector_collapsed_symbol == table.inspector_collapsed_symbol
        && old.inspector_expanded_symbol == table.inspector_expanded_symbol
        && headers_have_same_cell_text(old.header.as_ref(), table.header.as_ref())
}

pub(crate) fn reconcile_table(
    tree: &mut NodeTree,
    id: NodeId,
    table: &Table,
    rect: Rect,
) -> NodeId {
    let mut rect = rect;
    if matches!(table.width, Length::Auto) || matches!(table.height, Length::Auto) {
        let (w, h) = measure_table(table);
        if matches!(table.width, Length::Auto) {
            rect.w = w.min(rect.w);
        }
        if matches!(table.height, Length::Auto) {
            rect.h = h.min(rect.h);
        }
    }

    {
        let node = tree.node_mut(id);

        let (old_offset, scroll_override, cached_resolved_widths) =
            if let NodeKind::Table(old) = &mut node.kind {
                let reusable = can_reuse_resolved_widths(old, table);
                (
                    old.offset,
                    old.scroll_override,
                    reusable.then(|| std::mem::take(&mut old.resolved_widths)),
                )
            } else {
                (0, None, None)
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

        let resolved_widths = cached_resolved_widths.unwrap_or_else(|| derived_table_widths(table));

        node.rect = rect;
        node.children.clear();
        node.kind = NodeKind::Table(TableNode::from_table_with_resolved_widths(
            table.clone(),
            resolved_widths,
        ));

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

    #[test]
    fn reconcile_caches_resolved_widths_and_reuses_for_shared_rows() {
        use crate::widgets::ColumnWidth;
        use std::sync::Arc;

        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        let rows: Arc<[TableRow]> = Arc::from([
            TableRow::new(["1204", "lipan"]),
            TableRow::new(["2048", "cargo"]),
        ]);

        // 1. Explicit widths covering all columns stay None (zero allocation).
        let mut tree = NodeTree::new();
        let explicit_id = tree.alloc();
        let explicit_table = Table::new()
            .rows_arc(Arc::clone(&rows))
            .widths([ColumnWidth::Fixed(6), ColumnWidth::Fill(1)]);
        reconcile_table(&mut tree, explicit_id, &explicit_table, viewport);
        let NodeKind::Table(explicit_node) = &tree.node(explicit_id).kind else {
            panic!("expected table node");
        };
        assert!(explicit_node.resolved_widths.is_none());
        assert_eq!(
            explicit_node.effective_widths(),
            &[ColumnWidth::Fixed(6), ColumnWidth::Fill(1)]
        );

        // 2. Derived widths are cached on the node and moved without reallocation
        // when re-reconciling with shared rows_arc (e.g., on selection change).
        let derived_id = tree.alloc();
        let first = Table::new()
            .header(["PID", "Name"])
            .rows_arc(Arc::clone(&rows))
            .selected(Some(0));
        reconcile_table(&mut tree, derived_id, &first, viewport);
        let first_ptr = {
            let NodeKind::Table(node) = &tree.node(derived_id).kind else {
                panic!("expected table node");
            };
            assert_eq!(
                node.effective_widths(),
                &[ColumnWidth::Min(4), ColumnWidth::Min(5)]
            );
            node.resolved_widths
                .as_ref()
                .expect("derived widths cached")
                .as_ptr()
        };

        let second = Table::new()
            .header(["PID", "Name"])
            .rows_arc(Arc::clone(&rows))
            .selected(Some(1));
        reconcile_table(&mut tree, derived_id, &second, viewport);
        {
            let NodeKind::Table(node) = &tree.node(derived_id).kind else {
                panic!("expected table node");
            };
            assert_eq!(
                node.resolved_widths
                    .as_ref()
                    .expect("cached widths preserved")
                    .as_ptr(),
                first_ptr,
                "reconciling shared rows_arc should reuse cached resolved_widths without recomputing"
            );
        }

        // 3. Changing selection_symbol invalidates and recomputes resolved_widths.
        let third = Table::new()
            .header(["PID", "Name"])
            .rows_arc(Arc::clone(&rows))
            .selection_symbol(Some("> "))
            .selected(Some(1));
        reconcile_table(&mut tree, derived_id, &third, viewport);
        let NodeKind::Table(node) = &tree.node(derived_id).kind else {
            panic!("expected table node");
        };
        assert_eq!(
            node.effective_widths(),
            &[ColumnWidth::Min(6), ColumnWidth::Min(5)]
        );
    }
}
