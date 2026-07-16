use crate::app::input::drag;
use crate::app::input::scrollbar;
use crate::core::component::Component;
use crate::core::node::{NodeId, NodeKind, ScrollbarAxis};
#[cfg(feature = "terminal")]
use crate::utils::{GridPos, GridSelection};
use crate::widgets::internal::{ScrollAction, apply_scroll_action, scroll_metrics};

#[cfg(feature = "terminal")]
use crate::widgets::internal::{terminal_mouse_content_rect, terminal_selection_text};
use crate::widgets::{
    DocumentSelectEvent, DragCancelEvent, DragOverEvent, InputEvent, ProgressEvent, ScrollEvent,
    ScrollMetrics, TextAreaEvent, calc_scroll_view_window, normalize_input_offset,
};

use super::{ActiveDrag, AppRunner};

const STATIONARY_DRAG_AUTOSCROLL_INTERVAL_MS: u64 = 16;
const DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS: usize = 2;
const TEXT_SELECTION_DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS: usize = 1;
const DRAG_AUTOSCROLL_MAX_STEP_ROWS: usize = 6;

type ScrollViewDragAutoscrollTarget = (
    NodeId,
    usize,
    ScrollMetrics,
    Option<crate::callback::Callback<usize>>,
    Option<crate::callback::Callback<ScrollEvent>>,
);

type TextAreaDragAutoscrollTarget = (
    usize,
    ScrollMetrics,
    Option<crate::callback::Callback<usize>>,
    Option<crate::callback::Callback<ScrollEvent>>,
);

type DocumentViewDragAutoscrollTarget = (
    usize,
    ScrollMetrics,
    Option<crate::callback::Callback<ScrollEvent>>,
);

fn drag_edge_autoscroll_action(
    viewport_y: i16,
    viewport_h: u16,
    y: u16,
    edge_margin_rows: usize,
) -> Option<ScrollAction> {
    if viewport_h == 0 {
        return None;
    }

    let top = viewport_y;
    let bottom = viewport_y
        .saturating_add(viewport_h as i16)
        .saturating_sub(1);
    let y_i16 = y as i16;
    let edge_margin = edge_margin_rows.min(viewport_h as usize).max(1);

    let step_up = if y_i16 < top {
        Some((top - y_i16) as usize + edge_margin)
    } else {
        let top_zone_end = top.saturating_add(edge_margin as i16).saturating_sub(1);
        if y_i16 <= top_zone_end {
            let dist = (y_i16 - top).max(0) as usize;
            Some(edge_margin.saturating_sub(dist).max(1))
        } else {
            None
        }
    };

    if let Some(step) = step_up {
        return Some(ScrollAction::LineUp(
            step.min(DRAG_AUTOSCROLL_MAX_STEP_ROWS),
        ));
    }

    let step_down = if y_i16 > bottom {
        Some((y_i16 - bottom) as usize + edge_margin)
    } else {
        let bottom_zone_start = bottom.saturating_sub(edge_margin as i16).saturating_add(1);
        if y_i16 >= bottom_zone_start {
            let dist = (bottom - y_i16).max(0) as usize;
            Some(edge_margin.saturating_sub(dist).max(1))
        } else {
            None
        }
    }?;

    Some(ScrollAction::LineDown(
        step_down.min(DRAG_AUTOSCROLL_MAX_STEP_ROWS),
    ))
}

impl<C: Component> AppRunner<C> {
    pub(crate) fn clear_dnd_snapshot_cache(&self) {
        self.dnd_snapshot_cells.borrow_mut().take();
    }

    fn nearest_ancestor_drop_target(&self, start: NodeId) -> Option<NodeId> {
        drag::nearest_ancestor_drop_target(&self.core.tree, start)
    }

    fn drag_drop_target_is_compatible(
        &self,
        target_id: NodeId,
        drag_group: Option<&std::sync::Arc<str>>,
        payload: &dyn crate::widgets::DragPayload,
    ) -> bool {
        drag::drag_drop_target_is_compatible(&self.core.tree, target_id, drag_group, payload)
    }

    fn set_drag_source_dragging(&mut self, id: NodeId, dragging: bool) {
        drag::set_drag_source_dragging(&mut self.core.tree, id, dragging);
    }

    fn set_drop_target_highlighted(&mut self, id: NodeId, highlighted: bool) {
        drag::set_drop_target_highlighted(&mut self.core.tree, id, highlighted);
    }

    fn emit_drop_target_leave(
        &self,
        id: NodeId,
        payload: &std::sync::Arc<dyn crate::widgets::DragPayload>,
    ) {
        drag::emit_drop_target_leave(&self.core.tree, id, payload);
    }

    fn handle_drag_drop_move(&mut self, x: u16, y: u16) -> bool {
        let ActiveDrag::DragDrop(drag_state) = self.drag.active.clone() else {
            return false;
        };

        if !self.core.tree.is_valid(drag_state.source_id) {
            self.drag.clear();
            return true;
        }

        let hit = self.core.tree.hit_test(x as i16, y as i16);

        // Autoscroll any ScrollView that the cursor is near the edge of.
        let autoscrolled = hit.is_some_and(|h| self.maybe_autoscroll_scroll_view_drag_edge(h, y));

        // Track the nearest ScrollView under the cursor for stationary autoscroll ticks.
        let new_scroll_view_id = hit.and_then(|h| self.nearest_ancestor_scroll_view(h));

        let new_hover = hit
            .and_then(|h| self.nearest_ancestor_drop_target(h))
            .filter(|target_id| {
                self.drag_drop_target_is_compatible(
                    *target_id,
                    drag_state.drag_group.as_ref(),
                    drag_state.payload.as_ref(),
                )
            });

        // Always repaint when a visible preview tracks the pointer position.
        let has_visible_preview = !matches!(drag_state.preview, crate::widgets::DragPreview::None);

        if new_hover != drag_state.hovered_target {
            if let Some(old) = drag_state.hovered_target {
                self.set_drop_target_highlighted(old, false);
                self.emit_drop_target_leave(old, &drag_state.payload);
            }

            if let Some(new_id) = new_hover {
                self.set_drop_target_highlighted(new_id, true);
            }
        }

        let mut dirty = has_visible_preview || autoscrolled || self.drag.autoscroll_layout_dirty;

        if let Some(id) = new_hover
            && self.core.tree.is_valid(id)
            && let NodeKind::DropTarget(target) = &self.core.tree.node(id).kind
            && let Some(cb) = &target.on_drag_over
        {
            let rect = self.core.tree.node(id).rect;
            let top = rect.y.max(0) as u16;
            let local_y = y.saturating_sub(top);
            cb.emit(DragOverEvent {
                x,
                y,
                local_y,
                local_height: rect.h,
                payload: drag_state.payload.clone(),
            });
            dirty = true;
        }

        self.drag.active = ActiveDrag::DragDrop(crate::app::input::drag::DragDropDrag {
            hovered_target: new_hover,
            scroll_view_id: new_scroll_view_id,
            ..drag_state
        });
        dirty
    }

    fn try_activate_drag_drop(&mut self, x: u16, y: u16) -> bool {
        if !drag::try_activate_drag_drop(&mut self.core.tree, &mut self.mouse, &mut self.drag, x, y)
        {
            return false;
        }
        self.handle_drag_drop_move(x, y)
    }

    pub(crate) fn cancel_drag_drop(&mut self) -> bool {
        let ActiveDrag::DragDrop(drag) = self.drag.active.clone() else {
            return false;
        };

        if let Some(hovered) = drag.hovered_target {
            self.set_drop_target_highlighted(hovered, false);
            self.emit_drop_target_leave(hovered, &drag.payload);
        }

        self.set_drag_source_dragging(drag.source_id, false);

        if let Some(cb) = drag.on_cancel {
            cb.emit(DragCancelEvent {
                payload: drag.payload,
            });
        }

        self.clear_dnd_snapshot_cache();
        self.drag.clear();
        true
    }

    pub(crate) fn drag_preview_context(
        &self,
    ) -> Option<(u16, u16, &crate::app::input::drag::DragDropDrag)> {
        let ActiveDrag::DragDrop(drag) = &self.drag.active else {
            return None;
        };
        if !drag.started {
            return None;
        }
        self.drag.last_pointer_pos.map(|(x, y)| (x, y, drag))
    }

    pub(crate) fn nearest_ancestor_scroll_view(&self, start: NodeId) -> Option<NodeId> {
        let mut cur = Some(start);
        while let Some(id) = cur {
            if !self.core.tree.is_valid(id) {
                break;
            }
            let node = self.core.tree.node(id);
            if matches!(&node.kind, NodeKind::ScrollView(_)) {
                return Some(id);
            }
            cur = node.parent;
        }
        None
    }

    fn selection_scroll_view_drag_edge_autoscroll_target(
        &self,
        drag_node_id: NodeId,
        y: u16,
    ) -> Option<ScrollViewDragAutoscrollTarget> {
        self.scroll_view_drag_edge_autoscroll_target_with_margin(
            drag_node_id,
            y,
            TEXT_SELECTION_DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )
    }

    fn scroll_view_drag_edge_autoscroll_target_with_margin(
        &self,
        drag_node_id: NodeId,
        y: u16,
        edge_margin_rows: usize,
    ) -> Option<ScrollViewDragAutoscrollTarget> {
        let scroll_id = self.nearest_ancestor_scroll_view(drag_node_id)?;
        self.scroll_view_edge_autoscroll_target_for_with_margin(scroll_id, y, edge_margin_rows)
    }

    fn scroll_view_edge_autoscroll_target_for(
        &self,
        scroll_id: NodeId,
        y: u16,
    ) -> Option<ScrollViewDragAutoscrollTarget> {
        self.scroll_view_edge_autoscroll_target_for_with_margin(
            scroll_id,
            y,
            DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )
    }

    fn selection_scroll_view_edge_autoscroll_target_for(
        &self,
        scroll_id: NodeId,
        y: u16,
    ) -> Option<ScrollViewDragAutoscrollTarget> {
        self.scroll_view_edge_autoscroll_target_for_with_margin(
            scroll_id,
            y,
            TEXT_SELECTION_DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )
    }

    fn scroll_view_edge_autoscroll_target_for_with_margin(
        &self,
        scroll_id: NodeId,
        y: u16,
        edge_margin_rows: usize,
    ) -> Option<ScrollViewDragAutoscrollTarget> {
        if !self.core.tree.is_valid(scroll_id) {
            return None;
        }

        let (next, metrics, on_scroll_to, on_scroll) = {
            let node = self.core.tree.node(scroll_id);
            let NodeKind::ScrollView(scroll_view) = &node.kind else {
                return None;
            };

            let mut viewport = node
                .rect
                .inner(scroll_view.props.border, scroll_view.props.padding);
            if viewport.w == 0 || viewport.h == 0 {
                return None;
            }

            if scroll_view.show_scroll_indicators {
                if scroll_view.top_indicator {
                    viewport.y = viewport.y.saturating_add(1);
                    viewport.h = viewport.h.saturating_sub(1);
                }
                if scroll_view.bottom_indicator {
                    viewport.h = viewport.h.saturating_sub(1);
                }
            }
            if viewport.h == 0 {
                return None;
            }

            let action = drag_edge_autoscroll_action(viewport.y, viewport.h, y, edge_margin_rows)?;

            let total = scroll_view.content_height as usize;
            let viewport_h = scroll_view.viewport_height as usize;
            if viewport_h == 0 || total <= viewport_h {
                return None;
            }

            let visible_for_scroll = calc_scroll_view_window(
                scroll_view.offset,
                total,
                viewport_h,
                scroll_view.show_scroll_indicators,
            )
            .visible_rows;
            let metrics = ScrollMetrics {
                len: total,
                visible: visible_for_scroll,
                max_offset: scroll_view.max_offset,
            };

            let current = scroll_view.offset;
            let next = normalize_input_offset(
                current,
                apply_scroll_action(current, metrics, action).min(scroll_view.max_offset),
                total,
                viewport_h,
                scroll_view.show_scroll_indicators,
            );
            if next == current {
                return None;
            }

            (
                next,
                metrics,
                scroll_view.on_scroll_to.clone(),
                scroll_view.on_scroll.clone(),
            )
        };

        Some((scroll_id, next, metrics, on_scroll_to, on_scroll))
    }

    fn text_area_edge_autoscroll_target_for(
        &self,
        id: NodeId,
        y: u16,
    ) -> Option<TextAreaDragAutoscrollTarget> {
        if !self.core.tree.is_valid(id) {
            return None;
        }

        let node = self.core.tree.node(id);
        let NodeKind::TextArea(text_area) = &node.kind else {
            return None;
        };
        if text_area.disabled {
            return None;
        }

        let inner = node.rect.inner(text_area.border, text_area.padding);
        let viewport_h = text_area.geometry.content_viewport_h(false);
        let action = drag_edge_autoscroll_action(
            inner.y,
            viewport_h,
            y,
            TEXT_SELECTION_DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )?;
        let visible = viewport_h as usize;
        let total = text_area.visual_lines_count;
        if visible == 0 || total <= visible {
            return None;
        }

        let metrics = scroll_metrics(total, visible, text_area.scroll_offset);
        let current = text_area.scroll_offset;
        let next = apply_scroll_action(current, metrics, action).min(metrics.max_offset);
        if next == current {
            return None;
        }

        Some((
            next,
            metrics,
            text_area.on_scroll_to.clone(),
            text_area.on_scroll.clone(),
        ))
    }

    fn document_view_edge_autoscroll_target_for(
        &self,
        id: NodeId,
        y: u16,
    ) -> Option<DocumentViewDragAutoscrollTarget> {
        if !self.core.tree.is_valid(id) {
            return None;
        }

        let node = self.core.tree.node(id);
        let NodeKind::DocumentView(document_view) = &node.kind else {
            return None;
        };

        let inner = node.rect.inner(document_view.border, document_view.padding);
        let content_layout = document_view.content_layout(inner);
        if content_layout.content_width == 0 || content_layout.content_height == 0 {
            return None;
        }

        let action = drag_edge_autoscroll_action(
            content_layout.content_y,
            content_layout.content_height,
            y,
            TEXT_SELECTION_DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )?;
        let visible = content_layout.content_height as usize;
        let total = document_view.total_visual_lines;
        if visible == 0 || total <= visible {
            return None;
        }

        let metrics = ScrollMetrics {
            len: total,
            visible: visible.min(total),
            max_offset: total.saturating_sub(visible),
        };
        let current = document_view.scroll_offset;
        let next = apply_scroll_action(current, metrics, action).min(metrics.max_offset);
        if next == current {
            return None;
        }

        Some((next, metrics, document_view.on_scroll.clone()))
    }

    fn maybe_autoscroll_scroll_view_drag_edge(&mut self, drag_node_id: NodeId, y: u16) -> bool {
        self.maybe_autoscroll_scroll_view_drag_edge_with_margin(
            drag_node_id,
            y,
            DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )
    }

    fn maybe_autoscroll_selection_scroll_view_drag_edge(
        &mut self,
        drag_node_id: NodeId,
        y: u16,
    ) -> bool {
        self.maybe_autoscroll_scroll_view_drag_edge_with_margin(
            drag_node_id,
            y,
            TEXT_SELECTION_DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )
    }

    fn maybe_autoscroll_scroll_view_drag_edge_with_margin(
        &mut self,
        drag_node_id: NodeId,
        y: u16,
        edge_margin_rows: usize,
    ) -> bool {
        let Some(target) = self.scroll_view_drag_edge_autoscroll_target_with_margin(
            drag_node_id,
            y,
            edge_margin_rows,
        ) else {
            return false;
        };

        self.apply_scroll_view_autoscroll_target(target)
    }

    fn maybe_autoscroll_scroll_view_edge_for(&mut self, scroll_id: NodeId, y: u16) -> bool {
        self.maybe_autoscroll_scroll_view_edge_for_with_margin(
            scroll_id,
            y,
            DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )
    }

    fn maybe_autoscroll_selection_scroll_view_edge_for(
        &mut self,
        scroll_id: NodeId,
        y: u16,
    ) -> bool {
        self.maybe_autoscroll_scroll_view_edge_for_with_margin(
            scroll_id,
            y,
            TEXT_SELECTION_DRAG_AUTOSCROLL_EDGE_MARGIN_ROWS,
        )
    }

    fn maybe_autoscroll_scroll_view_edge_for_with_margin(
        &mut self,
        scroll_id: NodeId,
        y: u16,
        edge_margin_rows: usize,
    ) -> bool {
        let Some(target) =
            self.scroll_view_edge_autoscroll_target_for_with_margin(scroll_id, y, edge_margin_rows)
        else {
            return false;
        };

        self.apply_scroll_view_autoscroll_target(target)
    }

    fn apply_scroll_view_autoscroll_target(
        &mut self,
        target: ScrollViewDragAutoscrollTarget,
    ) -> bool {
        let (scroll_id, next, metrics, on_scroll_to, on_scroll) = target;

        if let NodeKind::ScrollView(scroll_view) = &mut self.core.tree.node_mut(scroll_id).kind {
            scroll_view.offset = next;
            scroll_view.scroll_override = Some(next);
            scroll_view.scroll_handler_dirty = true;
        } else {
            return false;
        }

        if let Some(cb) = on_scroll_to {
            cb.emit(next);
        } else if let Some(cb) = on_scroll {
            cb.emit(ScrollEvent {
                offset: next,
                metrics,
            });
        }

        self.drag.autoscroll_layout_dirty = true;
        true
    }

    fn maybe_autoscroll_text_area_drag_edge(&mut self, id: NodeId, y: u16) -> bool {
        let Some((next, metrics, on_scroll_to, on_scroll)) =
            self.text_area_edge_autoscroll_target_for(id, y)
        else {
            return false;
        };

        if let NodeKind::TextArea(text_area) = &mut self.core.tree.node_mut(id).kind {
            text_area.scroll_offset = next;
            text_area.scroll_override = Some(next);
            text_area.smooth_scroll.cancel_at(next);
            text_area.cancelled_scroll_to_line = text_area.scroll_to_line;
        } else {
            return false;
        }

        if let Some(cb) = on_scroll_to {
            cb.emit(next);
        } else if let Some(cb) = on_scroll {
            cb.emit(ScrollEvent {
                offset: next,
                metrics,
            });
        }

        true
    }

    fn maybe_autoscroll_document_view_drag_edge(&mut self, id: NodeId, y: u16) -> bool {
        let Some((next, metrics, on_scroll)) = self.document_view_edge_autoscroll_target_for(id, y)
        else {
            return false;
        };

        if let NodeKind::DocumentView(document_view) = &mut self.core.tree.node_mut(id).kind {
            document_view.scroll_offset = next;
            document_view.scroll_override = Some(next);
            document_view.smooth_scroll.cancel_at(next);
            document_view.cancelled_scroll_to_source_line = document_view.scroll_to_source_line;
        } else {
            return false;
        }

        if let Some(cb) = on_scroll {
            cb.emit(ScrollEvent {
                offset: next,
                metrics,
            });
        }

        true
    }

    pub(crate) fn stationary_drag_autoscroll_pending(&self) -> bool {
        let Some((_, y)) = self.drag.last_pointer_pos else {
            return false;
        };

        match &self.drag.active {
            ActiveDrag::TextArea(drag) => {
                self.core.tree.is_valid(drag.id)
                    && (self
                        .selection_scroll_view_drag_edge_autoscroll_target(drag.id, y)
                        .is_some()
                        || self
                            .text_area_edge_autoscroll_target_for(drag.id, y)
                            .is_some())
            }
            ActiveDrag::DocumentView(drag) => {
                if self.core.tree.is_valid(drag.id) {
                    self.selection_scroll_view_drag_edge_autoscroll_target(drag.id, y)
                        .is_some()
                        || self
                            .document_view_edge_autoscroll_target_for(drag.id, y)
                            .is_some()
                } else if let Some(sv_id) = drag.scroll_view_id {
                    self.selection_scroll_view_edge_autoscroll_target_for(sv_id, y)
                        .is_some()
                } else {
                    false
                }
            }
            ActiveDrag::DragDrop(drag) => drag.scroll_view_id.is_some_and(|sv_id| {
                self.scroll_view_edge_autoscroll_target_for(sv_id, y)
                    .is_some()
            }),
            _ => false,
        }
    }

    pub(crate) fn stationary_drag_autoscroll_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(STATIONARY_DRAG_AUTOSCROLL_INTERVAL_MS)
    }

    pub(crate) fn refresh_active_selection_drag_at(&mut self, x: u16, y: u16) -> bool {
        let (id, anchor, stable_anchor, shared_sel_id, sv_id) = match &self.drag.active {
            ActiveDrag::TextArea(drag) if self.core.tree.is_valid(drag.id) => {
                return self.handle_textarea_drag(x, y, drag.id, drag.anchor);
            }
            ActiveDrag::DocumentView(drag) => (
                drag.id,
                drag.anchor,
                drag.shared_drag_anchor.clone(),
                drag.shared_selection_id.clone(),
                drag.scroll_view_id,
            ),
            ActiveDrag::Input(drag) if self.core.tree.is_valid(drag.id) => {
                return self.handle_input_drag(x, drag.id, drag.anchor);
            }
            ActiveDrag::HexArea(drag) if self.core.tree.is_valid(drag.id) => {
                return self.handle_hex_area_drag(x, y, drag.id, drag.anchor);
            }
            _ => return false,
        };
        if self.core.tree.is_valid(id) {
            self.handle_document_view_drag(x, y, id, anchor, stable_anchor.as_ref())
        } else if let Some(shared_id) = shared_sel_id
            && let Some(sv_id) = sv_id
        {
            self.handle_offscreen_document_view_shared_drag(x, y, &shared_id, sv_id)
        } else {
            false
        }
    }

    pub(crate) fn refresh_active_selection_drag_from_last_pointer(&mut self) -> bool {
        let Some((x, y)) = self.drag.last_pointer_pos else {
            return false;
        };

        self.refresh_active_selection_drag_at(x, y)
    }

    pub(crate) fn tick_stationary_drag_autoscroll(&mut self) -> bool {
        self.drag.autoscroll_layout_dirty = false;

        let Some((x, y)) = self.drag.last_pointer_pos else {
            return false;
        };

        match &self.drag.active {
            ActiveDrag::TextArea(drag) => {
                let id = drag.id;
                if !self.core.tree.is_valid(id) {
                    return false;
                }

                let autoscrolled = self.maybe_autoscroll_selection_scroll_view_drag_edge(id, y)
                    || self.maybe_autoscroll_text_area_drag_edge(id, y);
                self.drag.last_autoscroll_tick = Some(web_time::Instant::now());
                if !autoscrolled {
                    return false;
                }

                self.refresh_active_selection_drag_at(x, y) || autoscrolled
            }
            ActiveDrag::DocumentView(drag) => {
                let id = drag.id;
                let scroll_view_id = drag.scroll_view_id;
                let autoscrolled = if self.core.tree.is_valid(id) {
                    self.maybe_autoscroll_selection_scroll_view_drag_edge(id, y)
                        || self.maybe_autoscroll_document_view_drag_edge(id, y)
                } else if let Some(sv_id) = scroll_view_id {
                    self.maybe_autoscroll_selection_scroll_view_edge_for(sv_id, y)
                } else {
                    false
                };
                self.drag.last_autoscroll_tick = Some(web_time::Instant::now());
                if !autoscrolled {
                    return false;
                }

                self.refresh_active_selection_drag_at(x, y) || autoscrolled
            }
            ActiveDrag::DragDrop(drag) => {
                let sv_id = drag.scroll_view_id;
                self.drag.last_autoscroll_tick = Some(web_time::Instant::now());
                let Some(sv_id) = sv_id else { return false };
                let autoscrolled = self.maybe_autoscroll_scroll_view_edge_for(sv_id, y);
                if autoscrolled {
                    self.handle_drag_drop_move(x, y) || autoscrolled
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn maybe_scroll_draggable_tab_bar_drag_edge(
        &mut self,
        x: u16,
        y: u16,
        drag_state: &crate::app::input::drag::DraggableTabBarDrag,
    ) -> bool {
        if !drag_state.started && x.abs_diff(drag_state.start_x) < drag_state.threshold {
            return false;
        }

        let target_id = self
            .core
            .tree
            .hit_test(x as i16, y as i16)
            .filter(|id| self.core.tree.is_valid(*id))
            .and_then(|id| match &self.core.tree.node(id).kind {
                NodeKind::DraggableTabBar(_) => Some(id),
                _ => None,
            })
            .unwrap_or(drag_state.id);

        if !self.core.tree.is_valid(target_id) {
            return false;
        }

        let next_offset = {
            let target_node = self.core.tree.node(target_id);
            let NodeKind::DraggableTabBar(target_bar) = &target_node.kind else {
                return false;
            };

            if target_bar.tabs.is_empty() {
                return false;
            }

            let target_inner = target_node
                .rect
                .inner(target_bar.border, target_bar.padding);
            if target_inner.w == 0 || target_inner.h == 0 {
                return false;
            }
            let y_dist = ((y as i16) - target_inner.y).unsigned_abs();
            let y_tolerance = target_inner.h.max(1) + 2;
            if y_dist >= y_tolerance {
                return false;
            }

            let max_col = target_inner.w.saturating_sub(1) as usize;
            let rel = (x as i32).saturating_sub(target_inner.x as i32);
            let view_col = rel.clamp(0, max_col as i32) as usize;

            let drag_disp_opts = target_bar.display_options();
            let drag_vp_opts = target_bar.viewport_options(target_inner.w as usize);
            let layout = crate::widgets::DraggableTabBar::viewport_layout(
                &target_bar.tabs,
                &drag_disp_opts,
                &drag_vp_opts,
            );

            let edge_margin = 2usize.min(max_col);
            let step_right = if view_col <= edge_margin && layout.hidden_left > 0 {
                Some(false)
            } else if view_col >= max_col.saturating_sub(edge_margin) && layout.hidden_right > 0 {
                Some(true)
            } else {
                None
            };

            let Some(step_right) = step_right else {
                return false;
            };

            let next = crate::widgets::DraggableTabBar::scroll_offset_for_step(
                &target_bar.tabs,
                &drag_disp_opts,
                &drag_vp_opts,
                step_right,
                crate::widgets::draggable_tab_bar::TAB_SCROLL_STEP_CHARS,
            );

            (next != target_bar.scroll_offset).then_some(next)
        };

        let Some(next_offset) = next_offset else {
            return false;
        };

        if let NodeKind::DraggableTabBar(node) = &mut self.core.tree.node_mut(target_id).kind {
            node.scroll_offset = next_offset;
            node.scroll_override = Some(next_offset);
            true
        } else {
            false
        }
    }

    pub(crate) fn handle_slider_drag(
        &mut self,
        x: u16,
        y: u16,
        id: NodeId,
        require_track_y: bool,
    ) -> bool {
        if let Some((value, on_change, on_click)) =
            drag::handle_slider_drag(&self.core.tree, x, y, id, require_track_y)
        {
            // Only emit if value actually changed
            let current_value = if let NodeKind::Slider(node) = &self.core.tree.node(id).kind {
                node.value
            } else {
                return false;
            };

            if (value - current_value).abs() < f64::EPSILON {
                return false; // Value hasn't changed, no need to render
            }

            let mut handled = false;
            if let Some(cb) = on_change {
                cb.emit(value);
                handled = true;
            }
            if let Some(cb) = on_click {
                cb.emit(value);
                handled = true;
            }
            handled
        } else {
            false
        }
    }

    pub(crate) fn handle_progress_drag(&mut self, x: u16, id: NodeId) -> bool {
        if let Some((progress, Some(cb))) = drag::handle_progress_drag(&self.core.tree, x, id) {
            // Only emit if progress actually changed
            let current_progress =
                if let NodeKind::ProgressBar(node) = &self.core.tree.node(id).kind {
                    node.progress
                } else {
                    return false;
                };

            if (progress - current_progress).abs() < f64::EPSILON {
                return false; // Progress hasn't changed, no need to render
            }

            cb.emit(ProgressEvent { progress });
            return true;
        }
        false
    }

    pub(crate) fn handle_splitter_drag(
        &mut self,
        x: u16,
        y: u16,
        drag: crate::app::input::drag::SplitterDrag,
    ) -> bool {
        let mut changed = self.apply_splitter_drag_target(
            x,
            y,
            drag.id,
            drag.handle,
            drag.start_pos,
            &drag.start_sizes,
        );
        if let Some(sec) = &drag.secondary {
            changed |= self.apply_splitter_drag_target(
                x,
                y,
                sec.id,
                sec.handle,
                sec.start_pos,
                &sec.start_sizes,
            );
        }
        changed
    }

    /// Apply one splitter's share of a drag update. A corner (junction) drag
    /// calls this twice: the primary splitter follows its own axis and the
    /// perpendicular secondary follows the other.
    fn apply_splitter_drag_target(
        &mut self,
        x: u16,
        y: u16,
        id: crate::core::node::NodeId,
        handle: usize,
        start_pos: i16,
        start_sizes: &[u16],
    ) -> bool {
        if !self.core.tree.is_valid(id) {
            return false;
        }

        let node = self.core.tree.node_mut(id);
        let NodeKind::Splitter(splitter) = &mut node.kind else {
            return false;
        };

        if handle + 1 >= start_sizes.len() {
            return false;
        }

        let delta = match splitter.orientation {
            crate::widgets::Orientation::Vertical => x as i16 - start_pos,
            crate::widgets::Orientation::Horizontal => y as i16 - start_pos,
        };

        let mut sizes = start_sizes.to_vec();
        let total = sizes[handle].saturating_add(sizes[handle + 1]);
        if total == 0 {
            return false;
        }

        let min = splitter.min_size.min(total);
        let max_left = total.saturating_sub(min);
        let min_left = min.min(max_left);

        let new_left =
            (sizes[handle] as i32 + delta as i32).clamp(min_left as i32, max_left as i32) as u16;
        let new_right = total.saturating_sub(new_left);

        if new_left == sizes[handle] && new_right == sizes[handle + 1] {
            return false;
        }

        sizes[handle] = new_left;
        sizes[handle + 1] = new_right;
        splitter.set_drag_sizes(sizes);
        splitter.active_handle = Some(handle);

        true
    }

    pub(crate) fn handle_draggable_tab_bar_drag(
        &mut self,
        x: u16,
        y: u16,
        drag_state: crate::app::input::drag::DraggableTabBarDrag,
    ) -> (bool, crate::app::input::drag::DraggableTabBarDrag) {
        let edge_scrolled = self.maybe_scroll_draggable_tab_bar_drag_edge(x, y, &drag_state);

        let Some((next_drag, reorder)) =
            drag::handle_draggable_tab_bar_drag(&self.core.tree, x, y, drag_state.clone())
        else {
            return (edge_scrolled, drag_state);
        };

        if let Some(event) = reorder {
            match event {
                drag::DraggableTabDragEvent::Reorder(event) => {
                    if self.core.tree.is_valid(next_drag.id)
                        && let NodeKind::DraggableTabBar(node) =
                            &self.core.tree.node(next_drag.id).kind
                        && let Some(cb) = &node.on_reorder
                    {
                        cb.emit(event);
                        return (true, next_drag);
                    }
                }
                drag::DraggableTabDragEvent::Transfer(event) => {
                    if let Some(cb) = &next_drag.on_transfer {
                        cb.emit(event);
                        return (true, next_drag);
                    }
                }
            }
        }

        (edge_scrolled, next_drag)
    }

    pub(crate) fn finish_draggable_tab_bar_drag(
        &mut self,
        drag_state: crate::app::input::drag::DraggableTabBarDrag,
    ) -> bool {
        let Some(event) = drag::finish_draggable_tab_bar_drag(drag_state.clone()) else {
            return false;
        };

        match event {
            drag::DraggableTabDragEvent::Reorder(event) => {
                if !self.core.tree.is_valid(drag_state.source_id) {
                    return false;
                }
                if let NodeKind::DraggableTabBar(node) =
                    &self.core.tree.node(drag_state.source_id).kind
                    && let Some(cb) = &node.on_reorder
                {
                    cb.emit(event);
                    return true;
                }
            }
            drag::DraggableTabDragEvent::Transfer(event) => {
                if let Some(cb) = &drag_state.on_transfer {
                    cb.emit(event);
                    return true;
                }
            }
        }

        false
    }

    /// Handle textarea mouse drag selection.
    pub(crate) fn handle_textarea_drag(
        &mut self,
        x: u16,
        y: u16,
        id: NodeId,
        anchor: usize,
    ) -> bool {
        if let Some((_value, cursor, anchor_opt, on_change, read_only)) =
            drag::handle_textarea_drag(&self.core.tree, x, y, id, anchor)
        {
            let has_on_change = on_change.is_some();
            // Only emit if cursor or anchor actually changed
            let (current_cursor, current_anchor) =
                if let NodeKind::TextArea(node) = &self.core.tree.node(id).kind {
                    (node.cursor, node.anchor)
                } else {
                    return false;
                };

            if cursor == current_cursor && anchor_opt == current_anchor {
                return false;
            }

            // Keep drag selection responsive by updating the live node state now;
            // the controlled on_change sync is emitted once on mouse release.
            let mut vim_motions = false;
            let mut on_vim_mode_change = None;
            if let NodeKind::TextArea(node) = &mut self.core.tree.node_mut(id).kind {
                vim_motions = node.vim_motions;
                on_vim_mode_change = node.on_vim_mode_change.clone();
                node.cursor = cursor;
                node.anchor = anchor_opt;
            }

            self.sync_textarea_vim_external_selection(
                crate::app::mouse_dispatch::TextareaVimExternalSelectionParams {
                    id,
                    vim_motions,
                    read_only,
                    has_on_change,
                    on_vim_mode_change: on_vim_mode_change.as_ref(),
                    cursor,
                    anchor: anchor_opt,
                },
            );

            if on_change.is_none() && read_only {
                self.widgets
                    .read_only_selection
                    .insert(id, (cursor, anchor_opt));
            }
            self.animation.reset_blink();
            return true;
        }
        false
    }

    pub(crate) fn finish_textarea_drag(&mut self, id: NodeId) {
        if !self.core.tree.is_valid(id) {
            return;
        }

        let NodeKind::TextArea(node) = &self.core.tree.node(id).kind else {
            return;
        };
        let value = node.value.clone();
        let cursor = node.cursor;
        let anchor = node.anchor;
        let on_change = node.on_change.clone();
        let on_editor_state_change = node.on_editor_state_change.clone();

        if let Some(cb) = on_change {
            cb.emit(TextAreaEvent {
                value: value.clone(),
                cursor,
                anchor,
            });
            if let Some(state_cb) = on_editor_state_change {
                state_cb.emit(crate::widgets::TextAreaStateChangeEvent {
                    reason: crate::widgets::TextAreaStateChangeReason::SelectionChange,
                    value,
                    cursor,
                    anchor,
                    edit: None,
                    vim_mode: None,
                });
            }
        }
    }

    /// Handle input mouse drag selection.
    pub(crate) fn handle_input_drag(&mut self, x: u16, id: NodeId, anchor: usize) -> bool {
        if let Some((value, cursor, anchor_opt, on_change, read_only)) =
            drag::handle_input_drag(&self.core.tree, x, id, anchor)
        {
            // Only emit if cursor actually changed
            let current_cursor = if let NodeKind::Input(node) = &self.core.tree.node(id).kind {
                node.cursor
            } else {
                return false;
            };

            if cursor == current_cursor {
                return false; // Cursor hasn't changed, no need to render
            }

            if let Some(cb) = on_change {
                cb.emit(InputEvent {
                    value,
                    cursor,
                    anchor: anchor_opt,
                });
            } else if read_only {
                self.widgets
                    .read_only_selection
                    .insert(id, (cursor, anchor_opt));
            }
            self.animation.reset_blink();
            return true;
        }
        false
    }

    /// Handle shared-selection drag when the anchor `DocumentView` has been
    /// swept off-screen.
    fn handle_offscreen_document_view_shared_drag(
        &mut self,
        x: u16,
        y: u16,
        shared_selection_id: &str,
        scroll_view_id: NodeId,
    ) -> bool {
        if !self.core.tree.is_valid(scroll_view_id) {
            return false;
        }

        let Some(anchor) = (match &self.drag.active {
            ActiveDrag::DocumentView(d) => d.shared_drag_anchor.as_ref(),
            _ => None,
        }) else {
            return false;
        };

        let Some(shared) = drag::handle_document_view_shared_linear_drag_offscreen(
            &self.core.tree,
            x,
            y,
            shared_selection_id,
            scroll_view_id,
            anchor,
        ) else {
            return false;
        };

        self.apply_shared_linear_drag_result(scroll_view_id, shared, None)
    }

    fn apply_shared_linear_drag_result(
        &mut self,
        scroll_view_id: NodeId,
        shared: drag::DocumentViewSharedLinearDragResult,
        on_select_hint_id: Option<NodeId>,
    ) -> bool {
        let drag::DocumentViewSharedLinearDragResult {
            updates,
            offscreen_patches,
            selected_text,
        } = shared;

        drag::apply_offscreen_shared_selection_patches(
            &mut self.core.tree,
            scroll_view_id,
            &offscreen_patches,
        );
        let had_patches = !offscreen_patches.is_empty();

        let mut changed = false;
        for update in &updates {
            if !self.core.tree.is_valid(update.id) {
                continue;
            }
            let (current_cursor, current_anchor, has_table_selection) =
                if let NodeKind::DocumentView(node) = &self.core.tree.node(update.id).kind {
                    (
                        node.selection_cursor,
                        node.selection_anchor,
                        node.table_rect_selection.is_some(),
                    )
                } else {
                    continue;
                };

            if update.cursor != current_cursor
                || update.anchor != current_anchor
                || has_table_selection
            {
                if let NodeKind::DocumentView(node) = &mut self.core.tree.node_mut(update.id).kind {
                    node.selection_cursor = update.cursor;
                    node.selection_anchor = update.anchor;
                    node.table_rect_selection = None;
                }
                changed = true;
            }
        }

        if !changed && !had_patches {
            return false;
        }

        let on_select = on_select_hint_id
            .filter(|id| self.core.tree.is_valid(*id))
            .and_then(|id| {
                if let NodeKind::DocumentView(node) = &self.core.tree.node(id).kind {
                    node.on_select.clone()
                } else {
                    None
                }
            })
            .or_else(|| {
                updates.iter().find_map(|u| {
                    if self.core.tree.is_valid(u.id)
                        && let NodeKind::DocumentView(node) = &self.core.tree.node(u.id).kind
                    {
                        node.on_select.clone()
                    } else {
                        None
                    }
                })
            });
        if let Some(cb) = on_select {
            cb.emit(DocumentSelectEvent { selected_text });
        }

        self.animation.reset_blink();
        true
    }

    /// Handle document view mouse drag selection.
    pub(crate) fn handle_document_view_drag(
        &mut self,
        x: u16,
        y: u16,
        id: NodeId,
        anchor: crate::app::input::drag::DocumentViewDragAnchor,
        shared_drag_anchor: Option<&crate::app::input::drag::SharedDocumentDragAnchor>,
    ) -> bool {
        #[cfg(feature = "diff-view")]
        if let crate::app::input::drag::DocumentViewDragAnchor::Linear(anchor_local) = anchor
            && let Some(diff_split) =
                drag::handle_diff_split_document_view_drag(&self.core.tree, x, y, id, anchor_local)
        {
            let mut changed = false;
            for update in diff_split.updates {
                if !self.core.tree.is_valid(update.id) {
                    continue;
                }
                let (current_cursor, current_anchor, current_table_rect_selection) =
                    if let NodeKind::DocumentView(node) = &self.core.tree.node(update.id).kind {
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

                if let NodeKind::DocumentView(node) = &mut self.core.tree.node_mut(update.id).kind {
                    node.selection_cursor = update.cursor;
                    node.selection_anchor = update.anchor;
                    node.table_rect_selection = None;
                }
                changed = true;
            }
            if changed {
                self.animation.reset_blink();
            }
            return changed;
        }

        if let crate::app::input::drag::DocumentViewDragAnchor::Linear(anchor_local) = anchor
            && let Some(shared) = drag::handle_document_view_shared_linear_drag(
                &self.core.tree,
                x,
                y,
                id,
                anchor_local,
                shared_drag_anchor,
            )
            && let Some(sv_id) = self.nearest_ancestor_scroll_view(id)
        {
            return self.apply_shared_linear_drag_result(sv_id, shared, Some(id));
        }

        if let Some((cursor, anchor_opt, table_rect_selection, on_select, selected_text)) =
            drag::handle_document_view_drag(&self.core.tree, x, y, id, anchor)
        {
            let (current_cursor, current_anchor, current_table_rect_selection) =
                if let NodeKind::DocumentView(node) = &self.core.tree.node(id).kind {
                    (
                        node.selection_cursor,
                        node.selection_anchor,
                        node.table_rect_selection.clone(),
                    )
                } else {
                    return false;
                };

            if cursor == current_cursor
                && anchor_opt == current_anchor
                && table_rect_selection == current_table_rect_selection
            {
                return false;
            }

            if let NodeKind::DocumentView(node) = &mut self.core.tree.node_mut(id).kind {
                node.selection_cursor = cursor;
                node.selection_anchor = anchor_opt;
                node.table_rect_selection = table_rect_selection;
            }

            if let Some(cb) = on_select
                && let Some(selected_text) = selected_text
            {
                cb.emit(DocumentSelectEvent { selected_text });
            }

            self.animation.reset_blink();
            return true;
        }
        false
    }

    /// Handle hex area mouse drag selection.
    pub(crate) fn handle_hex_area_drag(
        &mut self,
        x: u16,
        y: u16,
        id: NodeId,
        anchor: usize,
    ) -> bool {
        if let Some((cursor, anchor_opt, on_cursor_change)) =
            drag::handle_hex_area_drag(&self.core.tree, x, y, id, anchor)
        {
            let (current_cursor, current_anchor) =
                if let NodeKind::HexArea(node) = &self.core.tree.node(id).kind {
                    (node.cursor, node.anchor)
                } else {
                    return false;
                };

            if cursor == current_cursor && anchor_opt == current_anchor {
                return false;
            }

            if let Some(cb) = on_cursor_change {
                cb.emit(crate::widgets::HexAreaCursorEvent {
                    cursor,
                    anchor: anchor_opt,
                });
            }

            self.animation.reset_blink();
            return true;
        }
        false
    }

    /// Handle terminal mouse drag selection.
    #[cfg(feature = "terminal")]
    pub(crate) fn handle_terminal_drag(
        &mut self,
        x: u16,
        y: u16,
        drag: crate::app::input::drag::TerminalDrag,
    ) -> bool {
        crate::debug::internal_log!(
            "[terminal_drag] called x={} y={} anchor=({},{})",
            x,
            y,
            drag.anchor.row,
            drag.anchor.col
        );

        if !self.core.tree.is_valid(drag.id) {
            crate::debug::internal_log!("[terminal_drag] node invalid");
            return false;
        }

        let (content_rect, selection, lines, on_selection) = {
            let node = self.core.tree.node(drag.id);
            let NodeKind::Terminal(term) = &node.kind else {
                crate::debug::internal_log!("[terminal_drag] not a terminal node");
                return false;
            };

            let Some(content_rect) = terminal_mouse_content_rect(&self.core.tree, drag.id) else {
                crate::debug::internal_log!("[terminal_drag] content rect unavailable");
                return false;
            };

            (
                content_rect,
                term.selection.clone(),
                term.lines.clone(),
                term.on_selection.clone(),
            )
        };

        crate::debug::internal_log!(
            "[terminal_drag] content_rect: x={} y={} w={} h={}",
            content_rect.x,
            content_rect.y,
            content_rect.w,
            content_rect.h
        );

        if content_rect.w == 0 || content_rect.h == 0 {
            crate::debug::internal_log!("[terminal_drag] content_rect has zero size");
            return false;
        }

        let clamped_x = (x as i16).clamp(
            content_rect.x,
            content_rect.x.saturating_add(content_rect.w as i16 - 1),
        );
        let clamped_y = (y as i16).clamp(
            content_rect.y,
            content_rect.y.saturating_add(content_rect.h as i16 - 1),
        );

        let grid_col = (clamped_x - content_rect.x) as usize;
        let grid_row = (clamped_y - content_rect.y) as usize;

        crate::debug::internal_log!(
            "[terminal_drag] grid_col={} grid_row={}",
            grid_col,
            grid_row
        );

        let mut next = GridSelection::new(drag.anchor);
        next.extend_to(GridPos {
            row: grid_row,
            col: grid_col,
        });

        if selection.as_ref() == Some(&next) {
            crate::debug::internal_log!("[terminal_drag] selection unchanged");
            return false;
        }

        crate::debug::internal_log!(
            "[terminal_drag] creating selection: anchor=({},{}) cursor=({},{})",
            next.anchor.row,
            next.anchor.col,
            next.cursor.row,
            next.cursor.col
        );

        if let NodeKind::Terminal(term) = &mut self.core.tree.node_mut(drag.id).kind {
            term.selection = Some(next.clone());
        }
        if let Some(cb) = on_selection {
            let text = terminal_selection_text(&lines, &next);
            cb.emit(crate::widgets::TerminalSelectionEvent {
                selection: Some(next),
                text: Some(text),
            });
        }
        true
    }

    /// Get the current scrollbar offset for a node
    pub(crate) fn get_scrollbar_offset(&self, id: NodeId, axis: ScrollbarAxis) -> usize {
        if !self.core.tree.is_valid(id) {
            return 0;
        }
        let node = self.core.tree.node(id);
        match axis {
            ScrollbarAxis::Vertical => match &node.kind {
                NodeKind::List(list) => list.scroll_override.unwrap_or(list.offset),
                NodeKind::Table(table) => table.scroll_override.unwrap_or(table.offset),
                NodeKind::ScrollView(sv) => sv.scroll_override.unwrap_or(sv.scroll_offset as usize),
                NodeKind::TextArea(ta) => ta.scroll_override.unwrap_or(ta.scroll_offset),
                NodeKind::DocumentView(dv) => dv.scroll_override.unwrap_or(dv.scroll_offset),
                #[cfg(feature = "terminal")]
                NodeKind::Terminal(term) => term.scroll_override.unwrap_or(term.scrollback_offset),
                _ => 0,
            },
            ScrollbarAxis::Horizontal => match &node.kind {
                NodeKind::ScrollView(sv) => sv.h_scroll_override.unwrap_or(sv.h_offset),
                NodeKind::TextArea(ta) => ta.h_scroll_override.unwrap_or(ta.h_scroll_offset),
                NodeKind::DocumentView(dv) => dv.h_scroll_override.unwrap_or(dv.h_scroll_offset),
                _ => 0,
            },
        }
    }

    /// Dispatch a `MouseKind::Drag(MouseButton::Left)` event to the
    /// currently active drag handler.
    ///
    /// Returns `Some(true/false)` when a drag was active and handled (or not),
    /// `None` when no drag was active (`ActiveDrag::None`) so the caller can
    /// fall through to other handling.
    pub(crate) fn dispatch_active_drag(&mut self, x: u16, y: u16) -> Option<bool> {
        self.drag.autoscroll_layout_dirty = false;
        self.drag.remember_pointer(x, y);
        self.mouse.last_mouse = Some((x, y));

        if self.try_activate_drag_drop(x, y) {
            return Some(true);
        }

        match &self.drag.active {
            ActiveDrag::Slider(drag) => {
                let id = drag.id;
                if !self.core.tree.is_valid(id) {
                    self.drag.clear();
                    return Some(false);
                }
                Some(self.handle_slider_drag(x, y, id, false))
            }
            ActiveDrag::DraggableTabBar(drag) => {
                let drag = drag.clone();
                if !self.core.tree.is_valid(drag.id) {
                    self.clear_dnd_snapshot_cache();
                    self.drag.clear();
                    return Some(false);
                }
                // Suppress hover visuals while dragging tabs.
                let hover_cleared = self.mouse.hovered.take().is_some()
                    || self.mouse.hovered_item_index.take().is_some();
                let (handled, next_drag) = self.handle_draggable_tab_bar_drag(x, y, drag);
                let preview_dirty = next_drag.started
                    && (next_drag.preview_label.is_some()
                        || next_drag.preview_snapshot_anchor.is_some());
                self.drag.active = ActiveDrag::DraggableTabBar(next_drag);
                Some(handled || hover_cleared || preview_dirty)
            }
            ActiveDrag::Progress(drag) => {
                let id = drag.id;
                if !self.core.tree.is_valid(id) {
                    self.drag.clear();
                    return Some(false);
                }
                Some(self.handle_progress_drag(x, id))
            }
            ActiveDrag::DragDrop(_) => Some(self.handle_drag_drop_move(x, y)),
            ActiveDrag::Splitter(drag) => {
                let drag = drag.clone();
                if !self.core.tree.is_valid(drag.id) {
                    self.drag.clear();
                    return Some(false);
                }
                Some(self.handle_splitter_drag(x, y, drag))
            }
            ActiveDrag::Scrollbar(drag) => {
                let mut drag = drag.clone();
                if !self.core.tree.is_valid(drag.id)
                    && !scrollbar::rebind_drag_to_key(&self.core.tree, &mut drag)
                {
                    self.drag.clear();
                    return Some(false);
                }
                self.drag.active = ActiveDrag::Scrollbar(drag.clone());
                if self.drag.scrollbar_recalc {
                    if let Some(updated) =
                        scrollbar::start_drag(self.core.tree.node(drag.id), drag.axis, x, y)
                    {
                        self.drag.active = ActiveDrag::Scrollbar(updated);
                    }
                    self.drag.scrollbar_recalc = false;
                    return Some(true);
                }
                let ActiveDrag::Scrollbar(drag) = &self.drag.active else {
                    return Some(false);
                };

                // Get the current offset before handling drag
                let old_offset = self.get_scrollbar_offset(drag.id, drag.axis);

                let handled = scrollbar::handle_drag(
                    self.core.tree.node_mut(drag.id),
                    drag.axis,
                    x,
                    y,
                    drag.grab_offset,
                    drag.grab_subcell,
                );

                // Only return true if the offset actually changed
                if handled {
                    let new_offset = self.get_scrollbar_offset(drag.id, drag.axis);
                    if new_offset != old_offset {
                        scrollbar::remember_scroll_view_input_offset(&mut self.core.tree, drag.id);
                        return Some(true);
                    }
                }
                Some(false)
            }
            ActiveDrag::TextArea(drag) => {
                let (id, anchor) = (drag.id, drag.anchor);
                if !self.core.tree.is_valid(id) {
                    self.drag.clear();
                    return Some(false);
                }
                let autoscrolled = self.maybe_autoscroll_selection_scroll_view_drag_edge(id, y)
                    || self.maybe_autoscroll_text_area_drag_edge(id, y);
                Some(self.handle_textarea_drag(x, y, id, anchor) || autoscrolled)
            }
            ActiveDrag::DocumentView(drag) => {
                let id = drag.id;
                let anchor = drag.anchor;
                let stable = drag.shared_drag_anchor.clone();
                let shared_sel_id = drag.shared_selection_id.clone();
                let sv_id = drag.scroll_view_id;
                if self.core.tree.is_valid(id) {
                    let autoscrolled = self.maybe_autoscroll_selection_scroll_view_drag_edge(id, y)
                        || self.maybe_autoscroll_document_view_drag_edge(id, y);
                    Some(
                        self.handle_document_view_drag(x, y, id, anchor, stable.as_ref())
                            || autoscrolled,
                    )
                } else if let Some(shared_id) = shared_sel_id
                    && let Some(sv_id) = sv_id
                {
                    let autoscrolled =
                        self.maybe_autoscroll_selection_scroll_view_edge_for(sv_id, y);
                    Some(
                        self.handle_offscreen_document_view_shared_drag(x, y, &shared_id, sv_id)
                            || autoscrolled,
                    )
                } else {
                    self.drag.clear();
                    Some(false)
                }
            }
            ActiveDrag::Input(drag) => {
                let (id, anchor) = (drag.id, drag.anchor);
                if !self.core.tree.is_valid(id) {
                    self.drag.clear();
                    return Some(false);
                }
                Some(self.handle_input_drag(x, id, anchor))
            }
            ActiveDrag::HexArea(drag) => {
                let (id, anchor) = (drag.id, drag.anchor);
                if !self.core.tree.is_valid(id) {
                    self.drag.clear();
                    return Some(false);
                }
                Some(self.handle_hex_area_drag(x, y, id, anchor))
            }
            #[cfg(feature = "terminal")]
            ActiveDrag::Terminal(drag) => {
                let drag = *drag;
                if !self.core.tree.is_valid(drag.id) {
                    self.drag.clear();
                    return Some(false);
                }
                Some(self.handle_terminal_drag(x, y, drag))
            }
            ActiveDrag::None => None,
        }
    }
}
