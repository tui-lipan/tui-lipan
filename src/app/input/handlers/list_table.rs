//! List and Table keyboard and scroll-wheel handlers.

use crate::callback::KeyHandler;
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;
use crate::widgets::internal::{ScrollAction, apply_scroll_action};
use crate::widgets::list::utils::{
    calc_list_window, calc_list_window_for_items_with_indicators, visible_items_for_height,
};
use crate::widgets::table::{table_header_reserved_height, visible_rows_for_height};
use crate::widgets::{ListEvent, ScrollMetrics, TableEvent};

// ── Keyboard ────────────────────────────────────────────────────────────────

/// Handle keyboard input for a focused List node.
pub(crate) fn handle_list_key(tree: &mut NodeTree, id: NodeId, key: KeyEvent) -> bool {
    let handle_key = |handler: &KeyHandler| -> bool { handler.handle(key) };

    let NodeKind::List(node) = &tree.node(id).kind else {
        return false;
    };

    let len = node.items.len();
    let mut handled = false;

    if len == 0 {
        handled = node.on_key.as_ref().map(&handle_key).unwrap_or(false);
    } else {
        let selected_val = node.selected;
        let scroll_keys_val = node.scroll_keys;
        let on_select = node.on_select.clone();
        let on_activate = node.on_activate.clone();
        let on_key = node.on_key.clone();
        let border_val = node.border;
        let padding_val = node.padding;
        let offset_val = node.offset;
        let items = node.items.clone();

        let selected = crate::widgets::List::nearest_selectable_index(items.as_ref(), selected_val);

        if let Some(selected) = selected
            && let Some(cb) = on_select.as_ref()
        {
            // Handle PageUp/PageDown (hardcoded, not in ScrollKeymap)
            if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                let node_ref = tree.node(id);
                let inner = node_ref.rect.inner(border_val, padding_val);
                let visible_items = visible_items_for_height(items.as_ref(), offset_val, inner.h);
                let page_size = visible_items.saturating_sub(1).max(1);
                let target = match key.code {
                    KeyCode::PageDown => (selected + page_size).min(len.saturating_sub(1)),
                    KeyCode::PageUp => selected.saturating_sub(page_size),
                    _ => unreachable!(),
                };
                let next = match key.code {
                    KeyCode::PageDown => {
                        crate::widgets::List::selectable_at_or_after(items.as_ref(), target)
                            .or_else(|| {
                                crate::widgets::List::selectable_at_or_before(
                                    items.as_ref(),
                                    target,
                                )
                            })
                    }
                    KeyCode::PageUp => {
                        crate::widgets::List::selectable_at_or_before(items.as_ref(), target)
                            .or_else(|| {
                                crate::widgets::List::selectable_at_or_after(items.as_ref(), target)
                            })
                    }
                    _ => unreachable!(),
                };
                if let Some(next) = next
                    && next != selected
                {
                    cb.emit(ListEvent { index: next });
                    if let NodeKind::List(list_node) = &mut tree.node_mut(id).kind {
                        list_node.scroll_override = None;
                    }
                }
                handled = true;
            } else if let Some(next) = crate::widgets::List::next_selection(
                selected,
                items.as_ref(),
                &key,
                scroll_keys_val,
            ) {
                if next != selected {
                    cb.emit(ListEvent { index: next });
                    if let NodeKind::List(list_node) = &mut tree.node_mut(id).kind {
                        list_node.scroll_override = None;
                    }
                }
                handled = true;
            }
        }

        if !handled
            && matches!(key.code, KeyCode::Enter)
            && let Some(cb) = on_activate.as_ref()
            && let Some(selected) = selected
        {
            cb.emit(ListEvent { index: selected });
            handled = true;
        }

        if !handled {
            handled = on_key.as_ref().map(|h| h.handle(key)).unwrap_or(false);
        }
    }

    handled
}

/// Handle keyboard input for a focused Table node.
///
/// `rect` is the node's outer rectangle (from `tree.node(id).rect`), passed in
/// because the original dispatch reads it before entering the per-widget branch.
pub(crate) fn handle_table_key(tree: &mut NodeTree, id: NodeId, key: KeyEvent, rect: Rect) -> bool {
    let handle_key = |handler: &KeyHandler| -> bool { handler.handle(key) };

    let NodeKind::Table(node) = &tree.node(id).kind else {
        return false;
    };

    let len = node.rows.len();
    let mut handled = false;

    if len == 0 {
        handled = node.on_key.as_ref().map(&handle_key).unwrap_or(false);
    } else {
        let selected_val = node.selected;
        let scroll_keys_val = node.scroll_keys;
        let on_select = node.on_select.clone();
        let on_activate = node.on_activate.clone();
        let on_key = node.on_key.clone();
        let border_val = node.border;
        let padding_val = node.padding;
        let header_height =
            table_header_reserved_height(node.header.as_ref(), node.rows.len(), node.row_gap);

        let selected = selected_val.min(len.saturating_sub(1));

        if let Some(cb) = on_select.as_ref() {
            // Handle PageUp/PageDown (hardcoded, not in ScrollKeymap)
            if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                let inner = rect.inner(border_val, padding_val);
                let available_h = inner.h.saturating_sub(header_height);
                let visible =
                    visible_rows_for_height(&node.rows, node.offset, available_h, node.row_gap);
                let page_size = visible.saturating_sub(1).max(1);
                let next = match key.code {
                    KeyCode::PageDown => (selected + page_size).min(len.saturating_sub(1)),
                    KeyCode::PageUp => selected.saturating_sub(page_size),
                    _ => unreachable!(),
                };
                if next != selected {
                    cb.emit(TableEvent { index: next });
                    if let NodeKind::Table(table_node) = &mut tree.node_mut(id).kind {
                        table_node.scroll_override = None;
                    }
                }
                handled = true;
            } else if let Some(next) =
                crate::widgets::Table::next_selection(selected, len, &key, scroll_keys_val)
            {
                if next != selected {
                    cb.emit(TableEvent { index: next });
                    if let NodeKind::Table(table_node) = &mut tree.node_mut(id).kind {
                        table_node.scroll_override = None;
                    }
                }
                handled = true;
            }
        }

        if !handled
            && matches!(key.code, KeyCode::Enter)
            && let Some(cb) = on_activate.as_ref()
        {
            cb.emit(TableEvent { index: selected });
            handled = true;
        }

        if !handled {
            handled = on_key.as_ref().map(|h| h.handle(key)).unwrap_or(false);
        }
    }

    handled
}

// ── Scroll wheel ────────────────────────────────────────────────────────────

/// Handle scroll-wheel events for a List node.
pub(crate) fn handle_list_scroll(tree: &mut NodeTree, id: NodeId, action: ScrollAction) -> bool {
    // Immutable read phase - gather everything we need before mutating.
    let node = tree.node(id);
    let rect = node.rect;
    let NodeKind::List(list_node) = &node.kind else {
        return false;
    };

    if !list_node.scroll_wheel || list_node.disabled {
        return false;
    }

    let total = list_node.items.len();
    let inner = rect.inner(list_node.border, list_node.padding);
    let viewport_h = inner.h as usize;

    if viewport_h == 0 || total == 0 {
        return false;
    }

    let visible_items = visible_items_for_height(&list_node.items, list_node.offset, inner.h);
    // Mirror layout/scrollbar behavior: when indicators are enabled
    // and the list overflows, we reserve 1 row from the viewport
    // for the "N more" line(s) from a metrics perspective.
    let visible_for_scroll =
        if list_node.show_scroll_indicators && total > visible_items && visible_items > 0 {
            visible_items.saturating_sub(1)
        } else {
            visible_items
        }
        .min(total);

    let metrics = ScrollMetrics {
        len: total,
        visible: visible_for_scroll,
        max_offset: total.saturating_sub(visible_for_scroll),
    };
    let next = apply_scroll_action(list_node.offset, metrics, action).min(metrics.max_offset);
    let current_offset = list_node.offset;

    // Mutable write phase.
    if next != current_offset {
        let NodeKind::List(list_node) = &mut tree.node_mut(id).kind else {
            return false;
        };
        list_node.offset = next;
        list_node.scroll_override = Some(next);

        // Keep indicator flags in sync for immediate hit-testing.
        let (_s, e, t, b) = calc_list_window_for_items_with_indicators(
            list_node.offset,
            &list_node.items,
            viewport_h,
            list_node.show_scroll_indicators,
        );
        list_node.top_indicator = list_node.show_scroll_indicators && t;
        list_node.bottom_indicator = list_node.show_scroll_indicators && b;
        list_node.bottom_count = total.saturating_sub(e);
        true
    } else {
        false
    }
}

/// Handle scroll-wheel events for a Table node.
pub(crate) fn handle_table_scroll(tree: &mut NodeTree, id: NodeId, action: ScrollAction) -> bool {
    // Immutable read phase - gather everything we need before mutating.
    let node = tree.node(id);
    let rect = node.rect;
    let NodeKind::Table(table_node) = &node.kind else {
        return false;
    };

    if !table_node.scroll_wheel || table_node.disabled {
        return false;
    }

    let total = table_node.rows.len();
    let inner = rect.inner(table_node.border, table_node.padding);
    let header_h = table_header_reserved_height(
        table_node.header.as_ref(),
        table_node.rows.len(),
        table_node.row_gap,
    );
    let available_h = inner.h.saturating_sub(header_h);
    let viewport_rows = visible_rows_for_height(
        &table_node.rows,
        table_node.offset,
        available_h,
        table_node.row_gap,
    );

    if viewport_rows == 0 || total == 0 {
        return false;
    }

    let visible_for_scroll =
        if table_node.show_scroll_indicators && total > viewport_rows && viewport_rows > 0 {
            viewport_rows.saturating_sub(1)
        } else {
            viewport_rows
        }
        .min(total);

    let metrics = ScrollMetrics {
        len: total,
        visible: visible_for_scroll,
        max_offset: total.saturating_sub(visible_for_scroll),
    };
    let next = apply_scroll_action(table_node.offset, metrics, action).min(metrics.max_offset);
    let current_offset = table_node.offset;

    // Mutable write phase.
    if next != current_offset {
        let NodeKind::Table(table_node) = &mut tree.node_mut(id).kind else {
            return false;
        };
        table_node.offset = next;
        table_node.scroll_override = Some(next);

        let visible_after = visible_rows_for_height(
            &table_node.rows,
            table_node.offset,
            available_h,
            table_node.row_gap,
        );
        let (_s, e, t, b) = calc_list_window(table_node.offset, total, visible_after);
        table_node.top_indicator = table_node.show_scroll_indicators && t;
        table_node.bottom_indicator = table_node.show_scroll_indicators && b;
        table_node.bottom_count = total.saturating_sub(e);
        true
    } else {
        false
    }
}
