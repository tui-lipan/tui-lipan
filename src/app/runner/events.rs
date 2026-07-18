use crate::app::input::focus;
use crate::app::input::mouse;
use crate::app::mouse_dispatch;
use crate::core::component::Component;
#[cfg(feature = "terminal")]
use crate::core::event::MouseKind;
use crate::core::event::{KeyEvent, MouseEvent};
use crate::core::node::{NodeId, NodeKind};
use crate::runtime::BubbleKeyResult;
use crate::style::{Style, ThemeRole};
#[cfg(feature = "terminal")]
use crate::widgets::internal::terminal_mouse_content_rect;
#[cfg(feature = "terminal")]
use crate::widgets::{MouseMode, mouse_event_to_bytes};
use std::sync::Arc;

use super::AppRunner;

#[cfg(feature = "diff-view")]
fn node_has_diff_context_separator_hover(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::TextArea(text_area) => text_area
            .diff_context_separator_click
            .as_ref()
            .and_then(|config| config.hover_style)
            .is_some_and(|style| !style.is_empty()),
        NodeKind::DocumentView(document_view) => document_view
            .diff_context_separator_click
            .as_ref()
            .and_then(|config| config.hover_style)
            .is_some_and(|style| !style.is_empty()),
        _ => false,
    }
}

#[cfg(not(feature = "diff-view"))]
fn node_has_diff_context_separator_hover(_kind: &NodeKind) -> bool {
    false
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

#[derive(Clone)]
pub(crate) enum SelectionOwner {
    Node(NodeId),
    DocumentShared {
        scroll_view_id: NodeId,
        shared_selection_id: Arc<str>,
    },
}

impl<C: Component> AppRunner<C> {
    fn update_graph_node_hover(&mut self, id: NodeId, x: u16, y: u16) -> bool {
        let Some((index, event, cb)) = (|| {
            let node = self.core.tree.node(id);
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
            return self.mouse.hovered_item_index.take().is_some();
        };

        if self.mouse.hovered_item_index == Some(index) {
            return false;
        }
        self.mouse.hovered_item_index = Some(index);
        if let Some(cb) = cb {
            cb.emit(event);
        }
        true
    }

    fn update_sequence_item_hover(&mut self, id: NodeId, x: u16, y: u16) -> bool {
        let Some((hover_key, event, cb)) = (|| {
            let node = self.core.tree.node(id);
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
            return self.mouse.hovered_item_index.take().is_some();
        };

        if self.mouse.hovered_item_index == Some(hover_key) {
            return false;
        }
        self.mouse.hovered_item_index = Some(hover_key);
        if let Some(cb) = cb {
            cb.emit(event);
        }
        true
    }

    fn update_flowchart_item_hover(&mut self, id: NodeId, x: u16, y: u16) -> bool {
        let Some((hover_key, event, node_cb, edge_cb, subgraph_cb)) = (|| {
            let node = self.core.tree.node(id);
            let NodeKind::Flowchart(flowchart) = &node.kind else {
                return None;
            };
            let (local_x, local_y) =
                flowchart.local_content_point(node.rect, x as i16, y as i16)?;
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
            return self.mouse.hovered_item_index.take().is_some();
        };

        if self.mouse.hovered_item_index == Some(hover_key) {
            return false;
        }
        self.mouse.hovered_item_index = Some(hover_key);
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

    pub(crate) fn selection_owner_for_node(&self, start: NodeId) -> Option<SelectionOwner> {
        let mut cur = Some(start);
        while let Some(id) = cur {
            if !self.core.tree.is_valid(id) {
                break;
            }
            let node = self.core.tree.node(id);
            match &node.kind {
                NodeKind::Input(_) | NodeKind::TextArea(_) | NodeKind::HexArea(_) => {
                    return Some(SelectionOwner::Node(id));
                }
                NodeKind::DocumentView(doc) => {
                    if let Some(shared_selection_id) = doc.shared_selection_id.clone()
                        && let Some(scroll_view_id) = self.nearest_ancestor_scroll_view(id)
                    {
                        return Some(SelectionOwner::DocumentShared {
                            scroll_view_id,
                            shared_selection_id,
                        });
                    }
                    return Some(SelectionOwner::Node(id));
                }
                #[cfg(feature = "terminal")]
                NodeKind::Terminal(_) => return Some(SelectionOwner::Node(id)),
                _ => {
                    cur = node.parent;
                }
            }
        }
        None
    }

    fn selection_owner_matches_node(&self, owner: &SelectionOwner, id: NodeId) -> bool {
        if !self.core.tree.is_valid(id) {
            return false;
        }
        match owner {
            SelectionOwner::Node(keep_id) => *keep_id == id,
            SelectionOwner::DocumentShared {
                scroll_view_id,
                shared_selection_id,
            } => {
                let node = self.core.tree.node(id);
                let NodeKind::DocumentView(doc) = &node.kind else {
                    return false;
                };
                if !crate::app::input::drag::shared_selection_id_matches(
                    doc.shared_selection_id.as_deref(),
                    shared_selection_id.as_ref(),
                ) {
                    return false;
                }
                self.nearest_ancestor_scroll_view(id) == Some(*scroll_view_id)
            }
        }
    }

    pub(crate) fn clear_selectable_widget_selections(
        &mut self,
        keep: Option<SelectionOwner>,
    ) -> bool {
        let keep = keep.filter(|owner| match owner {
            SelectionOwner::Node(id) => self.core.tree.is_valid(*id),
            SelectionOwner::DocumentShared { scroll_view_id, .. } => {
                self.core.tree.is_valid(*scroll_view_id)
            }
        });
        let mut dirty = false;

        let before = self.widgets.read_only_selection.len();
        if let Some(SelectionOwner::Node(keep_id)) = keep.as_ref() {
            self.widgets
                .read_only_selection
                .retain(|id, _| *id == *keep_id);
        } else {
            self.widgets.read_only_selection.clear();
        }
        if self.widgets.read_only_selection.len() != before {
            dirty = true;
        }

        let ids: Vec<NodeId> = self.core.tree.iter().map(|node| node.id).collect();
        for id in ids {
            if !self.core.tree.is_valid(id) {
                continue;
            }
            if keep
                .as_ref()
                .is_some_and(|owner| self.selection_owner_matches_node(owner, id))
            {
                continue;
            }

            let mut clear_input = None;
            let mut clear_text_area = None;
            let mut clear_hex_area = None;
            let mut clear_document_view = false;
            #[cfg(feature = "terminal")]
            let mut clear_terminal = None;

            {
                let node = self.core.tree.node(id);
                match &node.kind {
                    NodeKind::Input(input)
                        if input.anchor.is_some_and(|anchor| anchor != input.cursor) =>
                    {
                        clear_input =
                            Some((input.on_change.clone(), input.value.clone(), input.cursor));
                    }
                    NodeKind::TextArea(text_area)
                        if text_area
                            .anchor
                            .is_some_and(|anchor| anchor != text_area.cursor) =>
                    {
                        clear_text_area = Some((
                            text_area.on_change.clone(),
                            text_area.value.clone(),
                            text_area.cursor,
                        ));
                    }
                    NodeKind::HexArea(hex_area)
                        if hex_area
                            .anchor
                            .is_some_and(|anchor| anchor != hex_area.cursor) =>
                    {
                        clear_hex_area = Some((hex_area.on_cursor_change.clone(), hex_area.cursor));
                    }
                    NodeKind::DocumentView(doc)
                        if (doc.table_rect_selection.is_some()
                            || doc
                                .selection_anchor
                                .is_some_and(|anchor| anchor != doc.selection_cursor)) =>
                    {
                        clear_document_view = true;
                    }
                    #[cfg(feature = "terminal")]
                    NodeKind::Terminal(term) if term.selection.is_some() => {
                        clear_terminal = Some(term.on_selection.clone());
                    }
                    _ => {}
                }
            }

            if let Some((on_change, value, cursor)) = clear_input {
                if let Some(cb) = on_change {
                    cb.emit(crate::widgets::InputEvent {
                        value,
                        cursor,
                        anchor: None,
                    });
                } else if let NodeKind::Input(node) = &mut self.core.tree.node_mut(id).kind {
                    node.anchor = None;
                }
                dirty = true;
            }

            if let Some((on_change, value, cursor)) = clear_text_area {
                if let Some(cb) = on_change {
                    cb.emit(crate::widgets::TextAreaEvent {
                        value,
                        cursor,
                        anchor: None,
                    });
                } else if let NodeKind::TextArea(node) = &mut self.core.tree.node_mut(id).kind {
                    node.anchor = None;
                }
                dirty = true;
            }

            if let Some((on_cursor_change, cursor)) = clear_hex_area {
                if let Some(cb) = on_cursor_change {
                    cb.emit(crate::widgets::HexAreaCursorEvent {
                        cursor,
                        anchor: None,
                    });
                } else if let NodeKind::HexArea(node) = &mut self.core.tree.node_mut(id).kind {
                    node.anchor = None;
                }
                dirty = true;
            }

            if clear_document_view {
                if let NodeKind::DocumentView(node) = &mut self.core.tree.node_mut(id).kind {
                    node.selection_anchor = None;
                    node.table_rect_selection = None;
                }
                dirty = true;
            }

            #[cfg(feature = "terminal")]
            if let Some(on_selection) = clear_terminal {
                if let NodeKind::Terminal(node) = &mut self.core.tree.node_mut(id).kind {
                    node.selection = None;
                }
                if let Some(cb) = on_selection {
                    cb.emit(crate::widgets::TerminalSelectionEvent {
                        selection: None,
                        text: None,
                    });
                }
                dirty = true;
            }
        }

        // Also clear offscreen stashed DocumentView selections in ScrollView
        // nodes, so that scrolling a cleared selection back into view doesn't
        // resurrect it.
        for id in self.core.tree.iter().map(|n| n.id).collect::<Vec<_>>() {
            if !self.core.tree.is_valid(id) {
                continue;
            }
            if let NodeKind::ScrollView(sv) = &mut self.core.tree.node_mut(id).kind
                && !sv.offscreen_doc_selections.is_empty()
            {
                match &keep {
                    Some(SelectionOwner::DocumentShared { scroll_view_id, .. })
                        if *scroll_view_id == id =>
                    {
                        // This ScrollView hosts the preserved shared group -
                        // keep all stashed offscreen selections so they
                        // survive scrolling back into view.
                    }
                    _ => {
                        sv.offscreen_doc_selections.clear();
                        dirty = true;
                    }
                }
            }
        }

        dirty
    }

    pub(crate) fn ensure_draggable_tab_bar_tab_visible(
        &mut self,
        id: NodeId,
        tab_index: usize,
    ) -> bool {
        if !self.core.tree.is_valid(id) {
            return false;
        }
        let rect = self.core.tree.node(id).rect;
        let NodeKind::DraggableTabBar(node_tabs) = &mut self.core.tree.node_mut(id).kind else {
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

    fn ensure_draggable_tab_bar_active_visible(&mut self, id: NodeId) -> bool {
        if !self.core.tree.is_valid(id) {
            return false;
        }
        let active = match &self.core.tree.node(id).kind {
            NodeKind::DraggableTabBar(node_tabs) => node_tabs.active,
            _ => return false,
        };
        self.ensure_draggable_tab_bar_tab_visible(id, active)
    }

    pub(crate) fn focus_for_node(&mut self, id: NodeId) -> bool {
        if self.focus.policy == crate::FocusPolicy::Manual {
            return false;
        }
        if !self.core.tree.is_valid(id) {
            return false;
        }
        let focusable = self.core.tree.node(id).is_focusable();

        if focusable {
            let focused_changed = self.focus.focused != Some(id);
            let mut dirty = false;
            if focused_changed {
                self.set_focus(id);
                dirty = true;
            }
            if focused_changed && self.ensure_draggable_tab_bar_active_visible(id) {
                dirty = true;
            }
            return dirty;
        }

        if let Some(descendant_id) = focus::find_first_focusable_descendant(&self.core.tree, id) {
            let focused_changed = self.focus.focused != Some(descendant_id);
            if focused_changed {
                self.set_focus(descendant_id);
                self.ensure_draggable_tab_bar_active_visible(descendant_id);
                return true;
            }
        }

        false
    }

    #[allow(dead_code)]
    pub(crate) fn bubble_key(&mut self, key: KeyEvent) -> BubbleKeyResult {
        self.core
            .bubble_key(self.focus.focused, self.focus.focused_key.as_ref(), key)
    }

    pub(crate) fn dispatch_mouse(&mut self, mouse: MouseEvent) -> bool {
        mouse_dispatch::dispatch_mouse_runner(self, mouse)
    }

    #[cfg(feature = "terminal")]
    pub(crate) fn forward_terminal_mouse(&mut self, mouse: MouseEvent) -> bool {
        let force_local = mouse.mods.shift;
        if force_local {
            return false;
        }

        let Some(hit) = self.core.tree.hit_test(mouse.x as i16, mouse.y as i16) else {
            return false;
        };
        if !self.core.tree.is_valid(hit) {
            return false;
        }
        if mouse::ancestor_mouse_region_captures_mods(&self.core.tree, hit, mouse.mods) {
            return false;
        }

        let (content_rect, encoding, on_mouse_forward) = {
            let node = self.core.tree.node(hit);
            let NodeKind::Terminal(term) = &node.kind else {
                return false;
            };

            if term.mouse_mode.mode == MouseMode::None {
                return false;
            }

            let should_forward = match term.mouse_mode.mode {
                MouseMode::X10 => matches!(mouse.kind, MouseKind::Down(_)),
                MouseMode::Normal => matches!(
                    mouse.kind,
                    MouseKind::Down(_)
                        | MouseKind::Up(_)
                        | MouseKind::Drag(_)
                        | MouseKind::ScrollUp
                        | MouseKind::ScrollDown
                ),
                MouseMode::AnyEvent => true,
                MouseMode::None => false,
            };
            if !should_forward {
                return false;
            }

            if matches!(mouse.kind, MouseKind::Moved) && term.mouse_mode.mode != MouseMode::AnyEvent
            {
                return false;
            }

            let Some(content_rect) = terminal_mouse_content_rect(&self.core.tree, hit) else {
                return false;
            };

            (
                content_rect,
                term.mouse_mode.encoding,
                term.on_mouse_forward.clone(),
            )
        };

        if content_rect.w == 0 || content_rect.h == 0 {
            return false;
        }
        if !content_rect.contains(mouse.x as i16, mouse.y as i16) {
            return false;
        }

        let Some(cb) = on_mouse_forward else {
            return false;
        };

        let offset = (content_rect.x.max(0) as u16, content_rect.y.max(0) as u16);
        if let Some(bytes) = mouse_event_to_bytes(mouse, encoding, offset) {
            // Clicks, drags, and scrolls focus the terminal they act on, but
            // plain motion must not: hovering over an any-event pane would
            // otherwise steal focus.
            if !matches!(mouse.kind, MouseKind::Moved) {
                let _ = self.focus_for_node(hit);
            }
            cb.emit(bytes);
            return true;
        }

        false
    }

    pub(crate) fn dispatch_mouse_move(&mut self, mouse: MouseEvent) -> bool {
        if self.mouse.last_mouse == Some((mouse.x, mouse.y)) {
            return false;
        }

        if !self.core.tree.has_mouse_move_handlers() {
            return false;
        }

        let Some(hit) = self
            .core
            .tree
            .mouse_move_test(mouse.x as i16, mouse.y as i16)
        else {
            return false;
        };

        let Some(action) =
            mouse::gather_mouse_move_action(&self.core.tree, hit, mouse.x, mouse.y, mouse.mods)
        else {
            return false;
        };

        action.cb.emit(action.event);
        // Emitting the move callback only queues a component message. The
        // message pump in the same runner tick decides whether that message
        // actually changed state and marks the corresponding dirty level. A
        // plain mouse move over an unchanged region must not force a paint by
        // itself; otherwise high-frequency terminal motion events can repaint
        // the whole tree even when the component returns `Update::none()`.
        false
    }

    pub(crate) fn update_hover_impl(&mut self, x: u16, y: u16, force_recompute: bool) -> bool {
        if !self.core.tree.has_hoverables() {
            self.mouse.last_mouse = Some((x, y));
            return false;
        }
        let prev_mouse = self.mouse.last_mouse;
        self.mouse.last_mouse = Some((x, y));
        if prev_mouse != Some((x, y)) {
            self.mouse.suppress_pointer_item_hover_nodes.clear();
        }
        let hovered = self
            .core
            .tree
            .hover_test(x as i16, y as i16)
            .filter(|id| mouse::should_hover(&self.core.tree, *id, x, y));
        let prev_hovered = self.mouse.hovered;
        let node_changed = hovered != prev_hovered;
        self.mouse.hovered = hovered;
        if node_changed {
            // Fire on_hover_change callbacks for enter/leave transitions
            use crate::core::node::NodeKind;

            // Fire leave callback for previously hovered node
            if let Some(prev_id) = prev_hovered
                && self.core.tree.is_valid(prev_id)
                && let NodeKind::MouseRegion(region) = &self.core.tree.node(prev_id).kind
                && let Some(cb) = region.on_hover_change.clone()
            {
                cb.emit(false);
            }

            // Fire enter callback for newly hovered node
            if let Some(new_id) = hovered
                && self.core.tree.is_valid(new_id)
                && let NodeKind::MouseRegion(region) = &self.core.tree.node(new_id).kind
                && let Some(cb) = region.on_hover_change.clone()
            {
                cb.emit(true);
            }

            self.mouse.hovered_item_index = None;
            if let Some(id) = hovered
                && self.core.tree.is_valid(id)
                && matches!(
                    self.core.tree.node(id).kind,
                    NodeKind::Graph(_) | NodeKind::SequenceDiagram(_) | NodeKind::Flowchart(_)
                )
            {
                let _ = if matches!(self.core.tree.node(id).kind, NodeKind::Graph(_)) {
                    self.update_graph_node_hover(id, x, y)
                } else if matches!(self.core.tree.node(id).kind, NodeKind::Flowchart(_)) {
                    self.update_flowchart_item_hover(id, x, y)
                } else {
                    self.update_sequence_item_hover(id, x, y)
                };
            }
            return true;
        }
        if !force_recompute && prev_mouse == Some((x, y)) {
            return false;
        }
        let Some(id) = self.mouse.hovered else {
            return false;
        };
        if !self.core.tree.is_valid(id) {
            return false;
        }
        // Check if this is a List/Table/Tabs with item-level hover
        let node = self.core.tree.node(id);
        let is_graph = matches!(node.kind, NodeKind::Graph(_));
        let is_sequence_diagram = matches!(node.kind, NodeKind::SequenceDiagram(_));
        let is_flowchart = matches!(node.kind, NodeKind::Flowchart(_));
        if is_graph {
            return self.update_graph_node_hover(id, x, y);
        }
        if is_sequence_diagram {
            return self.update_sequence_item_hover(id, x, y);
        }
        if is_flowchart {
            return self.update_flowchart_item_hover(id, x, y);
        }
        match &node.kind {
            NodeKind::List(list_node)
                if list_node
                    .item_hover_style
                    .resolves_non_empty(node.active_theme(), ThemeRole::ItemHover) =>
            {
                // Compute hovered item index
                let rect = node.rect;
                let inner = rect.inner(list_node.border, list_node.padding);
                if !inner.contains(x as i16, y as i16) {
                    if self.mouse.hovered_item_index.take().is_some() {
                        return true;
                    }
                    return false;
                }
                let top_reserved = if list_node.show_scroll_indicators && list_node.top_indicator {
                    1
                } else {
                    0
                };
                let rel_y = (y as i16).saturating_sub(inner.y) as usize;
                if rel_y < top_reserved {
                    if self.mouse.hovered_item_index.take().is_some() {
                        return true;
                    }
                    return false;
                }
                let item_row = rel_y.saturating_sub(top_reserved);
                let (start, end, _top, _bottom) =
                    crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                        list_node.offset,
                        &list_node.items,
                        inner.h as usize,
                        list_node.show_scroll_indicators,
                    );
                let visible_items = end.saturating_sub(start);
                let index = crate::widgets::list::utils::item_index_at_visual_line(
                    &list_node.items,
                    start,
                    item_row,
                    visible_items,
                )
                .filter(|idx| list_node.items[*idx].is_selectable());
                if index != self.mouse.hovered_item_index {
                    self.mouse.hovered_item_index = index;
                    return true;
                }
                false
            }
            NodeKind::Table(table_node)
                if table_node
                    .item_hover_style
                    .resolves_non_empty(node.active_theme(), ThemeRole::ItemHover) =>
            {
                let rect = node.rect;
                let inner = rect.inner(table_node.border, table_node.padding);
                if !inner.contains(x as i16, y as i16) {
                    if self.mouse.hovered_item_index.take().is_some() {
                        return true;
                    }
                    return false;
                }
                let top_reserved = if table_node.show_scroll_indicators && table_node.top_indicator
                {
                    1
                } else {
                    0
                };
                let header_height = crate::widgets::table::table_header_reserved_height(
                    table_node.header.as_ref(),
                    table_node.rows.len(),
                    table_node.row_gap,
                );
                let content_y = inner
                    .y
                    .saturating_add(top_reserved as i16)
                    .saturating_add(header_height as i16);
                if (y as i16) < content_y {
                    if self.mouse.hovered_item_index.take().is_some() {
                        return true;
                    }
                    return false;
                }
                let rel_y = (y as i16).saturating_sub(content_y) as u16;
                let index = crate::widgets::table::row_index_at_visual_offset(
                    &table_node.rows,
                    table_node.offset,
                    rel_y,
                    table_node.row_gap,
                );
                if index != self.mouse.hovered_item_index {
                    self.mouse.hovered_item_index = index;
                    return true;
                }
                false
            }
            NodeKind::Tabs(tabs_node)
                if tabs_node
                    .tab_hover_style
                    .resolves_non_empty(node.active_theme(), ThemeRole::ItemHover) =>
            {
                // For tabs, we'd need to compute which tab is hovered based on x position
                // For now, just track mouse position changes - tabs are typically few
                true
            }
            NodeKind::DraggableTabBar(tabs_node)
                if tabs_node
                    .tab_hover_style
                    .resolves_non_empty(node.active_theme(), ThemeRole::ItemHover)
                    || !tabs_node.close_hover_style.is_empty() =>
            {
                true
            }
            NodeKind::HexArea(hex_node) if !hex_node.hover_style.is_empty() => {
                let rect = node.rect;
                let index = crate::widgets::pointer_hit(
                    rect,
                    crate::widgets::HexAreaPointerHitArgs {
                        bytes_len: hex_node.bytes.len(),
                        cursor: hex_node.cursor,
                        bytes_per_row: hex_node.bytes_per_row,
                        show_offsets: hex_node.show_offsets,
                        show_ascii: hex_node.show_ascii,
                        scroll_offset: hex_node.scroll_offset,
                        border: hex_node.border,
                        padding: hex_node.padding,
                    },
                    x,
                    y,
                )
                .map(|hit| hit.index);
                if index != self.mouse.hovered_item_index {
                    self.mouse.hovered_item_index = index;
                    return true;
                }
                false
            }
            NodeKind::TextArea(text_area)
                if text_area.image_placeholder_hover_style != Style::default()
                    || text_area.sentinels.iter().any(|s| s.hover_style.is_some())
                    || node_has_diff_context_separator_hover(&node.kind) =>
            {
                true
            }
            NodeKind::DocumentView(_) if node_has_diff_context_separator_hover(&node.kind) => true,
            _ => false,
        }
    }

    pub(crate) fn update_hover(&mut self, x: u16, y: u16) -> bool {
        self.update_hover_impl(x, y, false)
    }

    pub(crate) fn refresh_hover_from_last_mouse(&mut self) -> bool {
        let Some((x, y)) = self.mouse.last_mouse else {
            return false;
        };

        self.update_hover_impl(x, y, true)
    }

    /// Optimised dispatch path for (coalesced) scroll-wheel events.
    ///
    /// Compared to the generic `dispatch_mouse`:
    /// * hover is only updated when the mouse has actually moved,
    /// * terminal forwarding is attempted only once (with the coalesced
    ///   count carried as `scroll_lines`),
    /// * the scroll count is forwarded to the tree handler so that a burst
    ///   of events results in a single tree mutation.
    pub(crate) fn dispatch_mouse_scroll(&mut self, mouse: MouseEvent, scroll_lines: u16) -> bool {
        let (x, y) = self.to_content_coords(mouse.x, mouse.y);
        let adjusted_mouse = MouseEvent { x, y, ..mouse };

        #[cfg(feature = "terminal")]
        if self.forward_terminal_mouse(adjusted_mouse) {
            return true;
        }

        let scroll_dirty = mouse::handle_scroll_wheel_n(
            &mut self.core.tree,
            adjusted_mouse,
            usize::from(scroll_lines),
            self.scroll_wheel_multiplier,
        );
        let selection_dirty = if scroll_dirty && self.drag.is_active() {
            self.drag.remember_pointer(x, y);
            self.refresh_active_selection_drag_at(x, y)
        } else {
            false
        };
        let hover_dirty = self.update_hover_impl(x, y, true);

        hover_dirty || scroll_dirty || selection_dirty
    }

    pub(crate) fn to_content_coords(&self, x: u16, y: u16) -> (u16, u16) {
        (x, y)
    }
}
