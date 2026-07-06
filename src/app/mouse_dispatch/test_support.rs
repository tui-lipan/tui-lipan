use crate::app::input::drag;
use crate::app::input::mouse;
use crate::app::input::scrollbar;
use crate::app::interaction_state::ActiveDrag;
use crate::core::component::Component;
use crate::core::event::MouseEvent;
use crate::core::node::NodeKind;
use crate::style::Style;
use crate::test_backend::TestBackend;

use super::{SelectionOwner, sync_textarea_vim_external_selection};

pub(crate) fn dispatch_mouse_move_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    mouse: MouseEvent,
) -> bool {
    if backend.mouse.last_mouse == Some((mouse.x, mouse.y)) {
        return false;
    }
    if !backend.core.tree.has_mouse_move_handlers() {
        return false;
    }
    let Some(hit) = backend
        .core
        .tree
        .mouse_move_test(mouse.x as i16, mouse.y as i16)
    else {
        return false;
    };
    let Some(action) =
        mouse::gather_mouse_move_action(&backend.core.tree, hit, mouse.x, mouse.y, mouse.mods)
    else {
        return false;
    };
    action.cb.emit(action.event);
    // The queued message, not the dispatch itself, owns the dirty verdict.
    false
}

pub(crate) fn update_hover_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    x: u16,
    y: u16,
    force_recompute: bool,
) -> bool {
    if !backend.core.tree.has_hoverables() {
        backend.mouse.last_mouse = Some((x, y));
        return false;
    }
    let prev = backend.mouse.last_mouse;
    backend.mouse.last_mouse = Some((x, y));
    if !force_recompute && prev == Some((x, y)) {
        return false;
    }
    let hovered = backend
        .core
        .tree
        .hover_test(x as i16, y as i16)
        .filter(|id| mouse::should_hover(&backend.core.tree, *id, x, y));
    let changed = backend.mouse.hovered != hovered;
    backend.mouse.hovered = hovered;
    if changed {
        backend.mouse.hovered_item_index = None;
        if let Some(id) = hovered {
            let item_hover_dirty = match &backend.core.tree.node(id).kind {
                NodeKind::Graph(_) => update_graph_node_hover_test_backend(backend, id, x, y),
                NodeKind::SequenceDiagram(_) => {
                    update_sequence_item_hover_test_backend(backend, id, x, y)
                }
                NodeKind::Flowchart(_) => {
                    update_flowchart_item_hover_test_backend(backend, id, x, y)
                }
                _ => false,
            };
            if item_hover_dirty {
                return true;
            }
        }
        return changed;
    }
    if !force_recompute && prev == Some((x, y)) {
        return false;
    }
    hovered.is_some_and(|id| match &backend.core.tree.node(id).kind {
        NodeKind::Graph(_) => update_graph_node_hover_test_backend(backend, id, x, y),
        NodeKind::SequenceDiagram(_) => update_sequence_item_hover_test_backend(backend, id, x, y),
        NodeKind::Flowchart(_) => update_flowchart_item_hover_test_backend(backend, id, x, y),
        NodeKind::TextArea(text_area)
            if text_area.image_placeholder_hover_style != Style::default()
                || text_area.sentinels.iter().any(|s| s.hover_style.is_some()) =>
        {
            true
        }
        _ => false,
    })
}

fn update_graph_node_hover_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    id: crate::core::node::NodeId,
    x: u16,
    y: u16,
) -> bool {
    let Some((index, event, cb)) = (|| {
        let node = backend.core.tree.node(id);
        let NodeKind::Graph(graph) = &node.kind else {
            return None;
        };
        let (local_x, local_y) = graph.local_content_point(node.rect, x as i16, y as i16)?;
        let (index, graph_node) = graph.hit_test(local_x, local_y)?;
        Some((
            index,
            crate::widgets::GraphNodeEvent {
                path: graph_node.path.clone(),
                label: graph_node.label.clone(),
            },
            graph.on_node_hover.clone(),
        ))
    })() else {
        return backend.mouse.hovered_item_index.take().is_some();
    };

    if backend.mouse.hovered_item_index == Some(index) {
        return false;
    }
    backend.mouse.hovered_item_index = Some(index);
    if let Some(cb) = cb {
        cb.emit(event);
    }
    true
}

fn sequence_item_hover_key(path: &crate::widgets::SequenceItemPath) -> usize {
    match path {
        crate::widgets::SequenceItemPath::Message(index) => *index,
        crate::widgets::SequenceItemPath::SelfMessage(index) => {
            1_000_000usize.saturating_add(*index)
        }
        crate::widgets::SequenceItemPath::Participant(index) => {
            2_000_000usize.saturating_add(*index)
        }
        crate::widgets::SequenceItemPath::Note(index) => 3_000_000usize.saturating_add(*index),
        crate::widgets::SequenceItemPath::Fragment(index) => 4_000_000usize.saturating_add(*index),
        crate::widgets::SequenceItemPath::Divider(index) => 5_000_000usize.saturating_add(*index),
    }
}

fn update_sequence_item_hover_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    id: crate::core::node::NodeId,
    x: u16,
    y: u16,
) -> bool {
    let Some((hover_key, event, cb)) = (|| {
        let node = backend.core.tree.node(id);
        let NodeKind::SequenceDiagram(sequence) = &node.kind else {
            return None;
        };
        let (local_x, local_y) = sequence.local_content_point(node.rect, x as i16, y as i16)?;
        let path = sequence.hit_test(local_x, local_y)?;
        let event = sequence.item_event(path.clone())?;
        Some((
            sequence_item_hover_key(&path),
            event,
            sequence.on_item_hover.clone(),
        ))
    })() else {
        return backend.mouse.hovered_item_index.take().is_some();
    };

    if backend.mouse.hovered_item_index == Some(hover_key) {
        return false;
    }
    backend.mouse.hovered_item_index = Some(hover_key);
    if let Some(cb) = cb {
        cb.emit(event);
    }
    true
}

fn update_flowchart_item_hover_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    id: crate::core::node::NodeId,
    x: u16,
    y: u16,
) -> bool {
    let Some((hover_key, event, node_cb, edge_cb, subgraph_cb)) = (|| {
        let node = backend.core.tree.node(id);
        let NodeKind::Flowchart(flowchart) = &node.kind else {
            return None;
        };
        let (local_x, local_y) = flowchart.local_content_point(node.rect, x as i16, y as i16)?;
        let (hover_key, path) = flowchart.hit_test(local_x, local_y)?;
        let event = flowchart.item_event(&path)?;
        Some((
            hover_key,
            event,
            flowchart.on_node_hover.clone(),
            flowchart.on_edge_hover.clone(),
            flowchart.on_subgraph_hover.clone(),
        ))
    })() else {
        return backend.mouse.hovered_item_index.take().is_some();
    };

    if backend.mouse.hovered_item_index == Some(hover_key) {
        return false;
    }
    backend.mouse.hovered_item_index = Some(hover_key);
    match event {
        crate::widgets::internal::FlowchartItemEvent::Node(event) => {
            if let Some(cb) = node_cb {
                cb.emit(event);
            }
        }
        crate::widgets::internal::FlowchartItemEvent::Edge(event) => {
            if let Some(cb) = edge_cb {
                cb.emit(event);
            }
        }
        crate::widgets::internal::FlowchartItemEvent::Subgraph(event) => {
            if let Some(cb) = subgraph_cb {
                cb.emit(event);
            }
        }
    }
    true
}

fn selection_owner_matches_node_test_backend(
    tree: &crate::core::node::NodeTree,
    owner: &SelectionOwner,
    id: crate::core::node::NodeId,
) -> bool {
    if !tree.is_valid(id) {
        return false;
    }
    match owner {
        SelectionOwner::Node(keep_id) => *keep_id == id,
        SelectionOwner::DocumentShared {
            scroll_view_id,
            shared_selection_id,
        } => {
            let node = tree.node(id);
            let NodeKind::DocumentView(doc) = &node.kind else {
                return false;
            };
            if !drag::shared_selection_id_matches(
                doc.shared_selection_id.as_deref(),
                shared_selection_id.as_ref(),
            ) {
                return false;
            }
            drag::nearest_ancestor_scroll_view(tree, id) == Some(*scroll_view_id)
        }
    }
}

pub(crate) fn clear_selectable_widget_selections_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    keep: Option<SelectionOwner>,
) -> bool {
    let keep = keep.filter(|owner| match owner {
        SelectionOwner::Node(id) => backend.core.tree.is_valid(*id),
        SelectionOwner::DocumentShared { scroll_view_id, .. } => {
            backend.core.tree.is_valid(*scroll_view_id)
        }
    });

    let mut dirty = false;
    let before = backend.read_only_selection.len();
    if let Some(SelectionOwner::Node(keep_id)) = keep.as_ref() {
        backend.read_only_selection.retain(|id, _| *id == *keep_id);
    } else {
        backend.read_only_selection.clear();
    }
    if backend.read_only_selection.len() != before {
        dirty = true;
    }

    let ids: Vec<_> = backend.core.tree.iter().map(|n| n.id).collect();
    for id in ids {
        if !backend.core.tree.is_valid(id) {
            continue;
        }
        if keep.as_ref().is_some_and(|owner| {
            selection_owner_matches_node_test_backend(&backend.core.tree, owner, id)
        }) {
            continue;
        }
        let node = backend.core.tree.node_mut(id);
        match &mut node.kind {
            NodeKind::Input(input) if input.anchor.is_some_and(|a| a != input.cursor) => {
                input.anchor = None;
                dirty = true;
            }
            NodeKind::TextArea(ta) if ta.anchor.is_some_and(|a| a != ta.cursor) => {
                ta.anchor = None;
                dirty = true;
            }
            NodeKind::HexArea(hex) if hex.anchor.is_some_and(|a| a != hex.cursor) => {
                hex.anchor = None;
                dirty = true;
            }
            NodeKind::DocumentView(doc)
                if (doc.table_rect_selection.is_some()
                    || doc
                        .selection_anchor
                        .is_some_and(|a| a != doc.selection_cursor)) =>
            {
                doc.selection_anchor = None;
                doc.table_rect_selection = None;
                dirty = true;
            }
            _ => {}
        }
    }

    dirty
}

pub(crate) fn handle_scrollbar_click_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    target: crate::core::node::ScrollbarTarget,
    x: u16,
    y: u16,
) -> bool {
    let drag = {
        let node = backend.core.tree.node(target.id);
        scrollbar::start_drag(node, target.axis, x, y)
    };
    if let Some(drag) = drag {
        let _ = backend.focus_for_node(target.id);
        backend.drag.active = ActiveDrag::Scrollbar(drag.clone());
        backend.drag.scrollbar_rect = Some(backend.core.tree.node(drag.id).rect);
        let handled = scrollbar::handle_drag(
            backend.core.tree.node_mut(drag.id),
            drag.axis,
            x,
            y,
            drag.grab_offset,
            drag.grab_subcell,
        );
        if handled {
            scrollbar::remember_scroll_view_input_offset(&mut backend.core.tree, drag.id);
        }
        return true;
    }
    false
}

fn ensure_draggable_tab_bar_tab_visible_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    id: crate::core::node::NodeId,
    tab_index: usize,
) -> bool {
    if !backend.core.tree.is_valid(id) {
        return false;
    }
    let rect = backend.core.tree.node(id).rect;
    let NodeKind::DraggableTabBar(node_tabs) = &mut backend.core.tree.node_mut(id).kind else {
        return false;
    };

    let inner = rect.inner(node_tabs.border, node_tabs.padding);
    if inner.w == 0 {
        return false;
    }

    let next = crate::widgets::DraggableTabBar::scroll_offset_to_reveal_tab(
        &node_tabs.tabs,
        &node_tabs.display_options(),
        &node_tabs.viewport_options(inner.w as usize),
        tab_index,
    );

    if next != node_tabs.scroll_offset {
        node_tabs.scroll_offset = next;
        node_tabs.scroll_override = Some(next);
        return true;
    }

    false
}

pub(crate) fn handle_draggable_tab_bar_click_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    action: crate::app::input::mouse::DraggableTabBarAction,
    x: u16,
    dirty: bool,
) -> bool {
    if action.overflow_scroll_step != 0 {
        let rect = if backend.core.tree.is_valid(action.node_id) {
            backend.core.tree.node(action.node_id).rect
        } else {
            return dirty;
        };
        if backend.core.tree.is_valid(action.node_id)
            && let NodeKind::DraggableTabBar(node_tabs) =
                &mut backend.core.tree.node_mut(action.node_id).kind
        {
            let inner = rect.inner(node_tabs.border, node_tabs.padding);
            let next = crate::widgets::DraggableTabBar::scroll_offset_for_step(
                &node_tabs.tabs,
                &node_tabs.display_options(),
                &node_tabs.viewport_options(inner.w as usize),
                action.overflow_scroll_step > 0,
                crate::widgets::draggable_tab_bar::TAB_SCROLL_BUTTON_STEP_CHARS,
            );
            if next != node_tabs.scroll_offset {
                node_tabs.scroll_offset = next;
                node_tabs.scroll_override = Some(next);
                return true;
            }
        }
    } else if action.close_hit {
        if let Some(cb) = action.on_close {
            cb.emit(crate::widgets::DraggableTabCloseEvent {
                index: action.tab_index,
            });
            return true;
        }
    } else if action.action_hit {
        if let Some(cb) = action.on_action {
            cb.emit(crate::widgets::DraggableTabActionEvent {
                index: action.tab_index,
            });
            return true;
        }
    } else {
        let mut handled = false;
        if ensure_draggable_tab_bar_tab_visible_test_backend(
            backend,
            action.node_id,
            action.tab_index,
        ) {
            handled = true;
        }
        if action.tab_index != action.active
            && let Some(cb) = action.on_change
        {
            cb.emit(crate::widgets::TabsEvent {
                index: action.tab_index,
            });
            handled = true;
        }
        if action.draggable
            && backend.core.tree.is_valid(action.node_id)
            && let NodeKind::DraggableTabBar(node_tabs) =
                &backend.core.tree.node(action.node_id).kind
            && (node_tabs.on_reorder.is_some() || node_tabs.on_transfer.is_some())
        {
            let preview_label = if node_tabs.drag_preview {
                node_tabs
                    .tabs
                    .get(action.tab_index)
                    .map(|t| t.label.clone())
            } else {
                None
            };
            backend.drag.active =
                ActiveDrag::DraggableTabBar(crate::app::input::drag::DraggableTabBarDrag {
                    source_id: action.node_id,
                    source_bar_id: action.bar_id.clone(),
                    source_index: action.tab_index,
                    id: action.node_id,
                    bar_id: action.bar_id.clone(),
                    current_index: action.tab_index,
                    pending_id: action.node_id,
                    pending_bar_id: action.bar_id.clone(),
                    pending_index: action.tab_index,
                    drag_group: action.drag_group,
                    on_transfer: action.on_transfer,
                    reorder_mode: action.reorder_mode,
                    threshold: action.drag_threshold,
                    start_x: x,
                    started: false,
                    preview_label,
                    preview_snapshot_anchor: None,
                });
            handled = true;
        }

        if handled {
            return true;
        }
    }
    false
}

pub(crate) fn handle_splitter_click_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    grab: crate::app::input::mouse::SplitterGrab,
    x: u16,
    y: u16,
) -> bool {
    if !backend.core.tree.is_valid(grab.node_id) {
        return false;
    }
    let (start_pos, start_sizes, orientation) = {
        let NodeKind::Splitter(node) = &mut backend.core.tree.node_mut(grab.node_id).kind else {
            return false;
        };
        let start_pos = match node.orientation {
            crate::widgets::Orientation::Vertical => x as i16,
            crate::widgets::Orientation::Horizontal => y as i16,
        };
        node.active_handle = Some(grab.handle);
        (start_pos, node.pane_sizes.clone(), node.orientation)
    };
    let secondary = crate::app::input::drag::find_junction_splitter(
        &backend.core.tree,
        grab.node_id,
        orientation,
        x,
        y,
    );
    if let Some(sec) = &secondary
        && let NodeKind::Splitter(node) = &mut backend.core.tree.node_mut(sec.id).kind
    {
        node.active_handle = Some(sec.handle);
    }
    backend.drag.active = ActiveDrag::Splitter(crate::app::input::drag::SplitterDrag {
        id: grab.node_id,
        handle: grab.handle,
        start_pos,
        start_sizes,
        secondary,
    });
    true
}

pub(crate) fn handle_list_click_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    hit: crate::core::node::NodeId,
    select: crate::app::input::mouse::ListSelect,
    x: u16,
    y: u16,
) -> bool {
    if select.len == 0 {
        return false;
    }

    let mut inner = select.rect.inner(select.border, select.padding);
    if select.scrollbar {
        let use_integrated = select.border
            && matches!(
                select.scrollbar_variant,
                crate::style::ScrollbarVariant::Integrated
            );
        let use_standalone = select.scrollbar && !use_integrated;
        if use_standalone && inner.w > 0 {
            inner.w = inner.w.saturating_sub(1);
        }
    }

    if !inner.contains(x as i16, y as i16) {
        return false;
    }

    let row = (y as i32).saturating_sub(inner.y as i32) as usize;
    let visible = inner.h as usize;

    if let NodeKind::List(node) = &mut backend.core.tree.node_mut(hit).kind {
        let total = node.items.len();
        if total == 0 || visible == 0 {
            return true;
        }

        let (start, end, has_top, has_bottom) =
            crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                node.offset,
                &node.items,
                visible,
                select.show_scroll_indicators,
            );
        let has_top = select.show_scroll_indicators && has_top;
        let has_bottom = select.show_scroll_indicators && has_bottom;
        let top_reserved = if has_top { 1 } else { 0 };
        let bottom_reserved = if has_bottom { 1 } else { 0 };

        if has_top && row == 0 {
            let mut next = node.offset.saturating_sub(1);
            if next == 1 {
                next = 0;
            }
            if next != node.offset {
                node.offset = next;
                node.scroll_override = Some(next);
            }

            let (_s, e, t, b) =
                crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                    node.offset,
                    &node.items,
                    visible,
                    select.show_scroll_indicators,
                );
            node.top_indicator = select.show_scroll_indicators && t;
            node.bottom_indicator = select.show_scroll_indicators && b;
            node.bottom_count = total.saturating_sub(e);
            return true;
        }

        if has_bottom && row == visible.saturating_sub(1) {
            let visible_items = crate::widgets::list::utils::visible_items_for_height(
                &node.items,
                node.offset,
                inner.h,
            );
            let visible_for_scroll = if select.show_scroll_indicators && total > visible_items {
                visible_items.saturating_sub(1)
            } else {
                visible_items
            };
            let max_offset = total.saturating_sub(visible_for_scroll);
            let mut next = node.offset.saturating_add(1).min(max_offset);
            if select.show_scroll_indicators && next == 1 {
                next = 2.min(max_offset);
            }
            if next != node.offset {
                node.offset = next;
                node.scroll_override = Some(next);
            }

            let (_s, e, t, b) =
                crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                    node.offset,
                    &node.items,
                    visible,
                    select.show_scroll_indicators,
                );
            node.top_indicator = select.show_scroll_indicators && t;
            node.bottom_indicator = select.show_scroll_indicators && b;
            node.bottom_count = total.saturating_sub(e);
            return true;
        }

        if row >= top_reserved && row < visible.saturating_sub(bottom_reserved) {
            let item_row = row.saturating_sub(top_reserved);
            let visible_items = end.saturating_sub(start);
            let Some(index) = crate::widgets::list::utils::item_index_at_visual_line(
                &node.items,
                start,
                item_row,
                visible_items,
            ) else {
                return true;
            };
            if index < total {
                if !node.items[index].is_selectable() {
                    return true;
                }
                node.scroll_override = Some(node.offset);
                let click_count = mouse::click_count_at(&mut backend.mouse.last_click, x, y, true);
                let is_double = click_count == 2;
                if let Some(cb) = &select.on_item_click {
                    cb.emit(crate::widgets::ListEvent { index });
                }
                backend
                    .mouse
                    .pointer_driven_item_hover_selection
                    .insert(hit);
                select.cb.emit(crate::widgets::ListEvent { index });
                if let Some(cb) = &select.on_activate
                    && (select.activate_on_click || is_double)
                {
                    cb.emit(crate::widgets::ListEvent { index });
                }
                return true;
            }
        }
    }

    false
}

pub(crate) fn handle_table_click_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    hit: crate::core::node::NodeId,
    select: crate::app::input::mouse::TableSelect,
    x: u16,
    y: u16,
) -> bool {
    if select.rows.is_empty() {
        return false;
    }

    let inner = select.rect.inner(select.border, select.padding);
    if !inner.contains(x as i16, y as i16) {
        return false;
    }

    if select.show_scroll_indicators && select.top_indicator && (y as i16) == inner.y {
        if let NodeKind::Table(table_node) = &mut backend.core.tree.node_mut(hit).kind {
            let next = table_node.offset.saturating_sub(1);
            if next != table_node.offset {
                table_node.offset = next;
                table_node.scroll_override = Some(next);
            }
        }
        return true;
    }

    if select.show_scroll_indicators
        && select.bottom_indicator
        && (y as i16) == inner.y.saturating_add(inner.h.saturating_sub(1) as i16)
    {
        if let NodeKind::Table(table_node) = &mut backend.core.tree.node_mut(hit).kind {
            let available_h = inner.h.saturating_sub(select.header_height);
            let visible_rows = crate::widgets::table::visible_rows_for_height(
                &table_node.rows,
                table_node.offset,
                available_h,
                table_node.row_gap,
            );
            let visible_for_scroll = if select.show_scroll_indicators
                && table_node.rows.len() > visible_rows
                && visible_rows > 0
            {
                visible_rows.saturating_sub(1)
            } else {
                visible_rows
            };
            let max_offset = table_node
                .rows
                .len()
                .saturating_sub(visible_for_scroll.max(1));
            let next = (table_node.offset + 1).min(max_offset);
            if next != table_node.offset {
                table_node.offset = next;
                table_node.scroll_override = Some(next);
            }
        }
        return true;
    }

    let top_reserved = if select.show_scroll_indicators && select.top_indicator {
        1
    } else {
        0
    };
    let content_y = inner
        .y
        .saturating_add(top_reserved as i16)
        .saturating_add(select.header_height as i16);
    if (y as i16) < content_y {
        return false;
    }

    let rel_y = (y as i32).saturating_sub(content_y as i32) as u16;
    let found = crate::widgets::table::row_index_at_visual_offset(
        &select.rows,
        select.offset,
        rel_y,
        select.row_gap,
    );

    if let Some(index) = found {
        if let NodeKind::Table(table_node) = &mut backend.core.tree.node_mut(hit).kind {
            table_node.scroll_override = Some(table_node.offset);
        }
        let click_count = mouse::click_count_at(&mut backend.mouse.last_click, x, y, true);
        let is_double = click_count == 2;
        backend
            .mouse
            .pointer_driven_item_hover_selection
            .insert(hit);
        select.cb.emit(crate::widgets::TableEvent { index });
        if is_double && let Some(cb) = &select.on_activate {
            cb.emit(crate::widgets::TableEvent { index });
        }
        return true;
    }

    false
}

pub(crate) fn handle_hex_area_click_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    hit: crate::core::node::NodeId,
    mouse: MouseEvent,
    x: u16,
    y: u16,
) -> bool {
    if let NodeKind::HexArea(hex) = &backend.core.tree.node(hit).kind
        && !hex.disabled
        && Some(hit) == backend.focused
        && let Some(hit_info) = crate::widgets::pointer_hit(
            backend.core.tree.node(hit).rect,
            crate::widgets::HexAreaPointerHitArgs {
                bytes_len: hex.bytes.len(),
                cursor: hex.cursor,
                bytes_per_row: hex.bytes_per_row,
                show_offsets: hex.show_offsets,
                show_ascii: hex.show_ascii,
                scroll_offset: hex.scroll_offset,
                border: hex.border,
                padding: hex.padding,
            },
            x,
            y,
        )
    {
        let on_cursor_change = hex.on_cursor_change.clone();
        let old_anchor = hex.anchor;
        let old_cursor = if hex.bytes.is_empty() {
            0
        } else {
            hex.cursor.min(hex.bytes.len().saturating_sub(1))
        };
        let new_cursor = hit_info.index;
        let new_anchor = if mouse.mods.shift {
            hex.anchor.or(Some(old_cursor))
        } else {
            None
        };

        let anchor_for_drag = new_anchor.unwrap_or(new_cursor);
        backend.drag.active = ActiveDrag::HexArea(crate::app::input::drag::HexAreaDrag {
            id: hit,
            anchor: anchor_for_drag,
        });
        backend.hex_pending_edit.remove(&hit);
        if let NodeKind::HexArea(hex_node) = &mut backend.core.tree.node_mut(hit).kind {
            hex_node.pending_high_nibble = None;
        }

        if let Some(cb) = on_cursor_change.as_ref()
            && (new_cursor != old_cursor || new_anchor != old_anchor)
        {
            cb.emit(crate::widgets::HexAreaCursorEvent {
                cursor: new_cursor,
                anchor: new_anchor,
            });
        }

        return true;
    }
    false
}

pub(crate) fn handle_document_view_click_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    hit: crate::core::node::NodeId,
    x: u16,
    y: u16,
) -> bool {
    if !backend.core.tree.is_valid(hit) {
        return false;
    }
    let NodeKind::DocumentView(doc) = &backend.core.tree.node(hit).kind else {
        return false;
    };
    let is_active = Some(hit) == backend.focused || !doc.focusable;
    if !is_active {
        return false;
    }
    let Some(cursor) = drag::document_view_cursor_from_coords(&backend.core.tree, x, y, hit) else {
        return false;
    };
    let cursor = cursor.min(doc.visual_cache.flat_text.len());
    let click_count = if doc.multi_click_select {
        mouse::click_count_at(&mut backend.mouse.last_click, x, y, true)
    } else {
        1
    };
    let table_hit = drag::document_view_table_cell_from_coords(&backend.core.tree, x, y, hit);
    if click_count == 1
        && let Some(hit_cell) = table_hit
    {
        backend.drag.active = ActiveDrag::DocumentView(crate::app::input::drag::DocumentViewDrag {
            id: hit,
            anchor: crate::app::input::drag::DocumentViewDragAnchor::TableCell {
                table_id: hit_cell.table_id,
                row_index: hit_cell.row_index,
                col_index: hit_cell.col_index,
                row_line_index: hit_cell.row_line_index,
                cell_line_anchor_byte: hit_cell.cell_line_anchor_byte,
            },
            shared_selection_id: doc.shared_selection_id.clone(),
            scroll_view_id: drag::nearest_ancestor_scroll_view(&backend.core.tree, hit),
            shared_drag_anchor: None,
        });
        if let NodeKind::DocumentView(doc_mut) = &mut backend.core.tree.node_mut(hit).kind {
            doc_mut.selection_cursor = cursor;
            doc_mut.selection_anchor = None;
            doc_mut.table_rect_selection = None;
        }
        clear_shared_document_selection_test_backend(backend, hit);
        return true;
    }

    let (new_cursor, new_anchor) = if click_count == 2 {
        let text = doc.visual_cache.flat_text.as_ref();
        let (start, end) = crate::app::input::text::word_at_byte(text, cursor, None);
        (end, Some(start))
    } else {
        (cursor, None)
    };

    backend.drag.active = ActiveDrag::DocumentView(crate::app::input::drag::DocumentViewDrag {
        id: hit,
        anchor: crate::app::input::drag::DocumentViewDragAnchor::Linear(
            new_anchor.unwrap_or(new_cursor),
        ),
        shared_selection_id: doc.shared_selection_id.clone(),
        scroll_view_id: drag::nearest_ancestor_scroll_view(&backend.core.tree, hit),
        shared_drag_anchor: None,
    });
    if let NodeKind::DocumentView(doc_mut) = &mut backend.core.tree.node_mut(hit).kind {
        doc_mut.selection_cursor = new_cursor;
        doc_mut.selection_anchor = new_anchor;
        doc_mut.table_rect_selection = None;
    }
    clear_shared_document_selection_test_backend(backend, hit);
    true
}

fn clear_shared_document_selection_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    hit: crate::core::node::NodeId,
) {
    let Some((shared_selection_id, scroll_view_id)): Option<(_, crate::core::node::NodeId)> =
        (|| {
            let node = backend.core.tree.node(hit);
            let NodeKind::DocumentView(doc) = &node.kind else {
                return None;
            };
            Some((
                doc.shared_selection_id.clone()?,
                drag::nearest_ancestor_scroll_view(&backend.core.tree, hit)?,
            ))
        })()
    else {
        return;
    };

    let ids: Vec<_> = backend.core.tree.iter().map(|node| node.id).collect();
    for other_id in ids {
        if other_id == hit || !backend.core.tree.is_valid(other_id) {
            continue;
        }

        let should_clear = {
            let node = backend.core.tree.node(other_id);
            let NodeKind::DocumentView(other_doc) = &node.kind else {
                continue;
            };
            drag::shared_selection_id_matches(
                other_doc.shared_selection_id.as_deref(),
                shared_selection_id.as_ref(),
            ) && drag::nearest_ancestor_scroll_view(&backend.core.tree, other_id)
                == Some(scroll_view_id)
        };

        if should_clear
            && let NodeKind::DocumentView(other_doc_mut) =
                &mut backend.core.tree.node_mut(other_id).kind
        {
            other_doc_mut.selection_anchor = None;
            other_doc_mut.table_rect_selection = None;
        }
    }

    if backend.core.tree.is_valid(scroll_view_id)
        && let NodeKind::ScrollView(sv) = &mut backend.core.tree.node_mut(scroll_view_id).kind
    {
        sv.offscreen_doc_selections.clear();
    }
}

pub(crate) fn dispatch_active_drag_test_backend<C: Component>(
    backend: &mut TestBackend<C>,
    x: u16,
    y: u16,
) -> Option<bool> {
    backend.drag.remember_pointer(x, y);
    backend.mouse.last_mouse = Some((x, y));
    match backend.drag.active.clone() {
        ActiveDrag::TextArea(drag_state) => {
            if let Some((value, cursor, anchor_opt, on_change, read_only)) =
                drag::handle_textarea_drag(
                    &backend.core.tree,
                    x,
                    y,
                    drag_state.id,
                    drag_state.anchor,
                )
            {
                let has_on_change = on_change.is_some();
                let mut vim_motions = false;
                let mut on_vim_mode_change = None;
                if let NodeKind::TextArea(node) =
                    &mut backend.core.tree.node_mut(drag_state.id).kind
                {
                    vim_motions = node.vim_motions;
                    on_vim_mode_change = node.on_vim_mode_change.clone();
                    node.cursor = cursor;
                    node.anchor = anchor_opt;
                }
                sync_textarea_vim_external_selection(
                    backend,
                    super::TextareaVimExternalSelectionParams {
                        id: drag_state.id,
                        vim_motions,
                        read_only,
                        has_on_change,
                        on_vim_mode_change: on_vim_mode_change.as_ref(),
                        cursor,
                        anchor: anchor_opt,
                    },
                );
                if let Some(cb) = on_change {
                    cb.emit(crate::widgets::TextAreaEvent {
                        value,
                        cursor,
                        anchor: anchor_opt,
                    });
                } else if read_only {
                    backend
                        .read_only_selection
                        .insert(drag_state.id, (cursor, anchor_opt));
                }
                return Some(true);
            }
            Some(false)
        }
        ActiveDrag::Input(drag_state) => {
            if let Some((value, cursor, anchor_opt, on_change, read_only)) =
                drag::handle_input_drag(&backend.core.tree, x, drag_state.id, drag_state.anchor)
            {
                if let Some(cb) = on_change {
                    cb.emit(crate::widgets::InputEvent {
                        value,
                        cursor,
                        anchor: anchor_opt,
                    });
                } else if read_only {
                    backend
                        .read_only_selection
                        .insert(drag_state.id, (cursor, anchor_opt));
                }
                return Some(true);
            }
            Some(false)
        }
        ActiveDrag::DocumentView(drag_state) => {
            #[cfg(feature = "diff-view")]
            if let crate::app::input::drag::DocumentViewDragAnchor::Linear(anchor_local) =
                drag_state.anchor
                && let Some(diff_split) = drag::handle_diff_split_document_view_drag(
                    &backend.core.tree,
                    x,
                    y,
                    drag_state.id,
                    anchor_local,
                )
            {
                let mut changed = false;
                for update in diff_split.updates {
                    if !backend.core.tree.is_valid(update.id) {
                        continue;
                    }
                    let (current_cursor, current_anchor, current_table_rect_selection) =
                        if let NodeKind::DocumentView(node) =
                            &backend.core.tree.node(update.id).kind
                        {
                            (
                                node.selection_cursor,
                                node.selection_anchor,
                                node.table_rect_selection.clone(),
                            )
                        } else {
                            continue;
                        };

                    if update.cursor == current_cursor
                        && update.anchor == current_anchor
                        && current_table_rect_selection.is_none()
                    {
                        continue;
                    }

                    if let NodeKind::DocumentView(node) =
                        &mut backend.core.tree.node_mut(update.id).kind
                    {
                        node.selection_cursor = update.cursor;
                        node.selection_anchor = update.anchor;
                        node.table_rect_selection = None;
                    }
                    changed = true;
                }
                return Some(changed);
            }

            if let crate::app::input::drag::DocumentViewDragAnchor::Linear(anchor_local) =
                drag_state.anchor
                && let Some(shared) = drag::handle_document_view_shared_linear_drag(
                    &backend.core.tree,
                    x,
                    y,
                    drag_state.id,
                    anchor_local,
                    drag_state.shared_drag_anchor.as_ref(),
                )
            {
                for update in shared.updates {
                    if !backend.core.tree.is_valid(update.id) {
                        continue;
                    }
                    if let NodeKind::DocumentView(node) =
                        &mut backend.core.tree.node_mut(update.id).kind
                    {
                        node.selection_cursor = update.cursor;
                        node.selection_anchor = update.anchor;
                        node.table_rect_selection = None;
                    }
                }
                if let Some(scroll_view_id) = drag_state.scroll_view_id {
                    drag::apply_offscreen_shared_selection_patches(
                        &mut backend.core.tree,
                        scroll_view_id,
                        &shared.offscreen_patches,
                    );
                }
                return Some(true);
            }

            if let Some((cursor, anchor_opt, table_rect_selection, on_select, selected_text)) =
                drag::handle_document_view_drag(
                    &backend.core.tree,
                    x,
                    y,
                    drag_state.id,
                    drag_state.anchor,
                )
            {
                if let NodeKind::DocumentView(node) =
                    &mut backend.core.tree.node_mut(drag_state.id).kind
                {
                    node.selection_cursor = cursor;
                    node.selection_anchor = anchor_opt;
                    node.table_rect_selection = table_rect_selection;
                }
                if let Some(cb) = on_select
                    && let Some(selected_text) = selected_text
                {
                    cb.emit(crate::widgets::DocumentSelectEvent { selected_text });
                }
                return Some(true);
            }
            Some(false)
        }
        ActiveDrag::HexArea(drag_state) => {
            if let Some((cursor, anchor_opt, on_cursor_change)) = drag::handle_hex_area_drag(
                &backend.core.tree,
                x,
                y,
                drag_state.id,
                drag_state.anchor,
            ) {
                if let Some(cb) = on_cursor_change {
                    cb.emit(crate::widgets::HexAreaCursorEvent {
                        cursor,
                        anchor: anchor_opt,
                    });
                }
                return Some(true);
            }
            Some(false)
        }
        ActiveDrag::Slider(drag_state) => {
            if let Some((value, on_change, _)) =
                drag::handle_slider_drag(&backend.core.tree, x, y, drag_state.id, false)
            {
                if let Some(cb) = on_change {
                    cb.emit(value);
                }
                return Some(true);
            }
            Some(false)
        }
        ActiveDrag::Progress(drag_state) => {
            if let Some((progress, on_change)) =
                drag::handle_progress_drag(&backend.core.tree, x, drag_state.id)
            {
                if let Some(cb) = on_change {
                    cb.emit(crate::widgets::ProgressEvent { progress });
                }
                return Some(true);
            }
            Some(false)
        }
        ActiveDrag::Scrollbar(drag_state) => {
            let mut drag_state = drag_state.clone();
            if !backend.core.tree.is_valid(drag_state.id)
                && !scrollbar::rebind_drag_to_key(&backend.core.tree, &mut drag_state)
            {
                backend.drag.clear();
                return Some(false);
            }
            backend.drag.active = ActiveDrag::Scrollbar(drag_state.clone());
            let handled = scrollbar::handle_drag(
                backend.core.tree.node_mut(drag_state.id),
                drag_state.axis,
                x,
                y,
                drag_state.grab_offset,
                drag_state.grab_subcell,
            );
            if handled {
                scrollbar::remember_scroll_view_input_offset(&mut backend.core.tree, drag_state.id);
            }
            Some(handled)
        }
        ActiveDrag::None => None,
        _ => {
            backend.drag.clear();
            Some(false)
        }
    }
}
