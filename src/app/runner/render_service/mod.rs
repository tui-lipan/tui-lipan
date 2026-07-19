use crossterm::{cursor::MoveTo, execute, style::Print};
#[cfg(any(feature = "devtools", feature = "profiling-tracing"))]
use web_time::Instant;

use ratatui::buffer::Cell;
use ratatui::layout::Position;
use std::cell::Cell as StdCell;
use std::collections::HashSet;
#[cfg(feature = "profiling-tracing")]
use tracing::trace_span;

use crate::Result;
#[cfg(feature = "terminal")]
use crate::backend::ratatui_backend::renderers::terminal_cursor_position;
use crate::backend::ratatui_backend::renderers::{
    input_cursor_position, text_area_cursor_position,
};
use crate::backend::ratatui_backend::{RenderContext, create_inline_terminal, render};
use crate::core::component::Component;
use crate::core::node::{NodeId, NodeKind};
use crate::layout::drag_source_layout_hint::{
    clear_drag_source_snapshot_collapse_key, set_drag_source_snapshot_collapse_key,
};
use crate::layout::measure::min_size_constrained;
use crate::style::{Rect, ThemeRole};
use crate::widgets::DragPreview;

use crate::app::input::{focus, scrollbar};

use super::{
    ActiveDrag, AppRunner, DirtyLevel, DirtyTracker,
    scroll_optimize::{capture_scroll_frames, replace_buffer_snapshot},
};

#[cfg(feature = "devtools")]
mod devtools;
mod incremental_scroll;
mod inline;

use inline::host_terminal_erase_scrollback_and_visible;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawMode {
    Full,
    LayoutOnly,
    PaintOnly,
}

const RENDER_STABILITY_MAX_PASSES: usize = 4;

#[derive(Clone, Copy, Debug)]
pub(super) struct ActiveTextAreaDragSnapshot {
    id: NodeId,
    cursor: usize,
    anchor: Option<usize>,
    scroll_offset: usize,
    scroll_override: Option<usize>,
    h_scroll_offset: usize,
    h_scroll_override: Option<usize>,
}

impl<C: Component> AppRunner<C> {
    /// Drain a pending focus request from the runtime env and apply it to
    /// `self.focus`. Returns `true` if a request was consumed.
    ///
    /// Called both from the event-loop drain site (where it also triggers a Full
    /// re-render) and immediately before each `restore_focus` call so that a
    /// request issued from a freshly-mounted component's `init()` lands on the
    /// same frame — without it, `restore_focus` would fall back to "first
    /// focusable" for one frame before the request is honored on the next tick.
    pub(super) fn apply_pending_focus_request(&mut self) -> bool {
        let Some(request) = self.core.ctx.take_focus_request() else {
            return false;
        };
        crate::app::focus_service::apply_focus_request(
            &self.core.tree,
            &mut self.focus.refs(),
            request,
        );
        true
    }

    pub(super) fn push_drag_layout_collapse_hint(&self) {
        let key = match &self.drag.active {
            ActiveDrag::DragDrop(d)
                if matches!(d.preview, DragPreview::SourceSnapshot)
                    && self.core.tree.is_valid(d.source_id) =>
            {
                self.core.tree.node(d.source_id).key.clone()
            }
            _ => None,
        };
        set_drag_source_snapshot_collapse_key(key);
    }

    pub(super) fn pop_drag_layout_collapse_hint(&self) {
        clear_drag_source_snapshot_collapse_key();
    }

    fn drain_render_time_messages(&mut self) -> Result<DirtyLevel> {
        let mut dirty = DirtyTracker::default();
        self.drain_messages_and_commands(&mut dirty)?;
        Ok(dirty.level())
    }

    pub(super) fn render_element_until_scroll_stable(&mut self, bounds: Rect) -> Result<()> {
        let text_area_drag_snapshot = self.active_text_area_drag_snapshot();
        for _pass in 0..RENDER_STABILITY_MAX_PASSES {
            let scroll_generations = self.core.scroll.view_generations();
            self.core.render_element(
                bounds,
                self.focus.focused,
                self.focus.focused_key.as_ref(),
                self.mouse.hovered,
            );

            let render_time_dirty = self.drain_render_time_messages()?;
            let needs_state_rerender =
                matches!(render_time_dirty, DirtyLevel::LayoutOnly | DirtyLevel::Full);
            // A scroll-generation change only affects view output when some
            // view() actually reads the data that moved (full TextArea metrics
            // vs. scrollbar visibility only); otherwise another pass would
            // rebuild an identical tree.
            let scroll_stable = !self
                .core
                .scroll
                .view_dependencies_stale(&scroll_generations);
            if scroll_stable && !needs_state_rerender {
                break;
            }

            #[cfg(debug_assertions)]
            if _pass + 1 == RENDER_STABILITY_MAX_PASSES {
                crate::debug::internal_log!(
                    "[tui-lipan] render stability cap reached: scroll_stable={} render_time_dirty={:?}",
                    scroll_stable,
                    render_time_dirty,
                );
            }
        }
        self.restore_active_text_area_drag_snapshot(text_area_drag_snapshot);
        Ok(())
    }

    fn seed_dnd_snapshot_cells_from_last_frame_if_needed(&self) {
        let anchor = match &self.drag.active {
            ActiveDrag::DragDrop(drag) => {
                if !matches!(drag.preview, DragPreview::SourceSnapshot) {
                    return;
                }
                drag.preview_snapshot_anchor
            }
            ActiveDrag::DraggableTabBar(drag) => {
                if !drag.started {
                    return;
                }
                drag.preview_snapshot_anchor
            }
            _ => return,
        };
        if self.dnd_snapshot_cells.borrow().is_some() {
            return;
        }
        let Some(anchor) = anchor else {
            return;
        };
        if anchor.is_empty() {
            return;
        }
        let Some(prev) = self.last_frame_snapshot.as_ref() else {
            return;
        };
        let src_rect = ratatui::layout::Rect::new(
            anchor.x.max(0) as u16,
            anchor.y.max(0) as u16,
            anchor.w,
            anchor.h,
        );
        if src_rect.width == 0 || src_rect.height == 0 {
            return;
        }
        let mut cells: Vec<Cell> =
            Vec::with_capacity((src_rect.width as usize) * (src_rect.height as usize));
        let area = prev.area();
        for dy in 0..src_rect.height {
            for dx in 0..src_rect.width {
                let sx = src_rect.x + dx;
                let sy = src_rect.y + dy;
                let cell = if sx < area.width && sy < area.height {
                    prev.cell(Position::new(sx, sy))
                        .cloned()
                        .unwrap_or_default()
                } else {
                    Cell::default()
                };
                cells.push(cell);
            }
        }
        *self.dnd_snapshot_cells.borrow_mut() = Some((src_rect.width, src_rect.height, cells));
    }

    pub(super) fn active_text_area_drag_snapshot(&self) -> Option<ActiveTextAreaDragSnapshot> {
        let ActiveDrag::TextArea(drag) = &self.drag.active else {
            return None;
        };
        let id = drag.id;
        if !self.core.tree.is_valid(id) {
            return None;
        }

        let NodeKind::TextArea(text_area) = &self.core.tree.node(id).kind else {
            return None;
        };

        Some(ActiveTextAreaDragSnapshot {
            id,
            cursor: text_area.cursor,
            anchor: text_area.anchor,
            scroll_offset: text_area.scroll_offset,
            scroll_override: text_area.scroll_override,
            h_scroll_offset: text_area.h_scroll_offset,
            h_scroll_override: text_area.h_scroll_override,
        })
    }

    pub(super) fn restore_active_text_area_drag_snapshot(
        &mut self,
        snapshot: Option<ActiveTextAreaDragSnapshot>,
    ) {
        let Some(snapshot) = snapshot else {
            return;
        };
        let ActiveDrag::TextArea(drag) = &self.drag.active else {
            return;
        };
        if drag.id != snapshot.id || !self.core.tree.is_valid(snapshot.id) {
            return;
        }

        let NodeKind::TextArea(text_area) = &mut self.core.tree.node_mut(snapshot.id).kind else {
            return;
        };

        let max_scroll_offset = text_area
            .visual_lines_count
            .saturating_sub(text_area.geometry.content_viewport_h(false) as usize);
        let max_h_scroll_offset = text_area
            .max_line_width
            .saturating_sub(text_area.geometry.content_width);
        let max_cursor = text_area.value.len();

        text_area.cursor = snapshot.cursor.min(max_cursor);
        text_area.anchor = snapshot.anchor.map(|anchor| anchor.min(max_cursor));
        text_area.scroll_offset = snapshot.scroll_offset.min(max_scroll_offset);
        text_area.scroll_override = snapshot
            .scroll_override
            .map(|offset| offset.min(max_scroll_offset));
        text_area.h_scroll_offset = snapshot.h_scroll_offset.min(max_h_scroll_offset);
        text_area.h_scroll_override = snapshot
            .h_scroll_override
            .map(|offset| offset.min(max_h_scroll_offset));
    }

    pub(super) fn incremental_cursor_position(&self) -> Option<Position> {
        let id = self.focus.focused?;
        if !self.core.tree.is_valid(id) {
            return None;
        }

        let node = self.core.tree.node(id);
        let scroll_clip = crate::backend::ratatui_backend::render::scroll_view_clip_rect(
            &self.core.tree,
            node.parent,
            ratatui::layout::Rect::new(0, 0, 0, 0),
        );

        match &node.kind {
            NodeKind::Input(input) => {
                if input.disabled || input.read_only {
                    return None;
                }
                input_cursor_position(
                    &input.value,
                    input.cursor,
                    input.anchor,
                    crate::backend::ratatui_backend::renderers::text_area::InputCursorDecor {
                        prefix: input.prefix.as_deref(),
                        suffix: input.suffix.as_deref(),
                        truncate_head: input.truncate_head,
                        mask: input.mask,
                    },
                    crate::backend::ratatui_backend::renderers::text_area::InputCursorLayout {
                        border: input.border,
                        padding: input.padding,
                        rect: node.rect,
                        clip_rect: scroll_clip,
                    },
                )
            }
            NodeKind::TextArea(text_area) => {
                if text_area.disabled || text_area.read_only {
                    return None;
                }
                text_area_cursor_position(
                    &text_area.value,
                    crate::backend::ratatui_backend::renderers::text_area::TextAreaCursorInput {
                        cursor: text_area.cursor,
                        anchor: text_area.anchor,
                        allow_selection_cursor: text_area.vim_motions,
                    },
                    crate::backend::ratatui_backend::renderers::text_area::TextAreaCursorVimCtx {
                        search_feedback: text_area.vim_search_feedback.as_ref(),
                        visual_line_caret: text_area.vim_visual_line_caret,
                    },
                    crate::backend::ratatui_backend::renderers::text_area::TextAreaCursorLayout {
                        wrap: text_area.wrap,
                        h_scroll_offset: text_area.h_scroll_offset,
                        scroll_offset: text_area.scroll_offset,
                        border: text_area.border,
                        padding: text_area.padding,
                        line_numbers: text_area.line_numbers,
                        min_line_number_width: text_area.min_line_number_width,
                        rect: node.rect,
                        clip_rect: scroll_clip,
                        parent_integrated_v_edge: self
                            .core
                            .tree
                            .parent_frame_integrated_v_edge(id)
                            .unwrap_or(false),
                        parent_integrated_h_edge: self
                            .core
                            .tree
                            .parent_frame_integrated_h_edge(id)
                            .unwrap_or(false),
                    },
                    crate::backend::ratatui_backend::renderers::text_area::TextAreaCursorScrollCtx {
                        scrollbar: text_area.scrollbar,
                        scrollbar_variant: text_area.scrollbar_variant,
                        scrollbar_gap: text_area.scrollbar_gap,
                        h_scrollbar: text_area.h_scrollbar,
                        h_scrollbar_variant: text_area.h_scrollbar_variant,
                        max_line_width: text_area.max_line_width,
                        read_only: text_area.read_only,
                    },
                    crate::backend::ratatui_backend::renderers::text_area::TextAreaCursorExtrasCtx {
                        visual_cache: &text_area.visual_cache,
                        value_hash: text_area.content_hash,
                        peer_hash: crate::widgets::hash_peer_source_lines(
                            text_area.peer_source_lines.as_ref(),
                        ),
                        images_count: text_area.images.len(),
                        image_mode: text_area.image_mode,
                        image_placeholder: &text_area.image_placeholder,
                        sentinels: &text_area.sentinels,
                        virtual_texts: &text_area.virtual_texts,
                        gutter_col_width: text_area.gutter_col_width,
                        gutter_gap: text_area.gutter_gap,
                        geometry: &text_area.geometry,
                        tab_stop: text_area.tab_display_width as usize,
                    },
                )
            }
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(terminal) => {
                if !terminal.cursor_visible || terminal.scrollback_offset != 0 {
                    return None;
                }
                if terminal
                    .selection
                    .as_ref()
                    .is_some_and(|selection| !selection.is_empty())
                {
                    return None;
                }
                terminal_cursor_position(
                    terminal,
                    node.rect,
                    scroll_clip,
                    self.core
                        .tree
                        .parent_frame_integrated_v_edge(id)
                        .unwrap_or(false),
                )
            }
            _ => None,
        }
    }

    /// After keyboard- or programmatic-driven list/table selection changes, ignore pointer
    /// position for `item_hover_style` until the mouse moves again. Pointer row clicks opt out
    /// via `MouseTrackingState::pointer_driven_item_hover_selection`.
    /// Also suppresses hover for newly-appearing nodes (e.g. a modal just opened) so the
    /// hover style is never applied based on stale pointer position - only after the mouse
    /// actually moves.
    /// `Tree` is covered indirectly: it renders a `List`, which is tracked here by `NodeId`.
    fn sync_pointer_item_hover_suppression(&mut self) {
        let tree = &self.core.tree;
        let mut list_ids = HashSet::new();
        let mut table_ids = HashSet::new();

        for node in tree.iter() {
            match &node.kind {
                NodeKind::List(list)
                    if list
                        .item_hover_style
                        .resolves_non_empty(node.active_theme(), ThemeRole::ItemHover) =>
                {
                    list_ids.insert(node.id);
                    match self.last_seen_list_selection.insert(node.id, list.selected) {
                        None => {
                            // Newly appeared node (e.g. modal just opened) - suppress until
                            // the mouse actually moves.
                            self.mouse.suppress_pointer_item_hover_nodes.insert(node.id);
                        }
                        Some(prev)
                            if prev != list.selected
                                && !self
                                    .mouse
                                    .pointer_driven_item_hover_selection
                                    .contains(&node.id) =>
                        {
                            self.mouse.suppress_pointer_item_hover_nodes.insert(node.id);
                        }
                        _ => {}
                    }
                }
                NodeKind::Table(table)
                    if table
                        .item_hover_style
                        .resolves_non_empty(node.active_theme(), ThemeRole::ItemHover) =>
                {
                    table_ids.insert(node.id);
                    match self
                        .last_seen_table_selection
                        .insert(node.id, table.selected)
                    {
                        None => {
                            self.mouse.suppress_pointer_item_hover_nodes.insert(node.id);
                        }
                        Some(prev)
                            if prev != table.selected
                                && !self
                                    .mouse
                                    .pointer_driven_item_hover_selection
                                    .contains(&node.id) =>
                        {
                            self.mouse.suppress_pointer_item_hover_nodes.insert(node.id);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        self.last_seen_list_selection
            .retain(|id, _| list_ids.contains(id));
        self.last_seen_table_selection
            .retain(|id, _| table_ids.contains(id));
        self.mouse.pointer_driven_item_hover_selection.clear();
    }

    pub(super) fn render(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        #[cfg(feature = "profiling-tracing")]
        let _render_span = trace_span!("app.render_full").entered();

        #[cfg(feature = "devtools")]
        let total_start = Instant::now();

        if self.surface.is_transcript() && self.surface.inline.transcript_reset_pending {
            return self.render_expanded_transcript(terminal);
        }

        let size = terminal.size()?;

        let bounds = self.content_bounds(size.width, size.height);

        #[cfg(feature = "devtools")]
        self.install_devtools_overlay();

        #[cfg(feature = "devtools")]
        let reconcile_start = Instant::now();
        #[cfg(feature = "profiling-tracing")]
        let _reconcile_span = trace_span!("app.reconcile_full").entered();
        self.push_drag_layout_collapse_hint();
        let reconcile_result = self.render_element_until_scroll_stable(bounds);
        self.pop_drag_layout_collapse_hint();
        reconcile_result?;

        // Auto-height inline viewports follow the content: re-measure after
        // reconcile and re-reconcile at the settled height. Capped in case a
        // view keeps changing its natural height in response to the viewport.
        let mut bounds = bounds;
        for _ in 0..RENDER_STABILITY_MAX_PASSES {
            let Some(new_bounds) = self.sync_inline_auto_height(terminal, bounds)? else {
                break;
            };
            bounds = new_bounds;
            self.push_drag_layout_collapse_hint();
            let reconcile_result = self.render_element_until_scroll_stable(bounds);
            self.pop_drag_layout_collapse_hint();
            reconcile_result?;
        }

        self.dirty_component_scopes.clear();
        self.dirty_scope_set.clear();
        #[cfg(feature = "devtools")]
        let reconcile_duration = reconcile_start.elapsed();
        #[cfg(feature = "profiling-tracing")]
        drop(_reconcile_span);

        let draw_duration = self.finalize_after_reconcile(terminal, DrawMode::Full)?;
        #[cfg(feature = "devtools")]
        self.record_devtools_frame_metrics(
            DrawMode::Full,
            total_start.elapsed(),
            reconcile_duration,
            draw_duration,
        );
        #[cfg(not(feature = "devtools"))]
        let _ = draw_duration;
        Ok(())
    }

    pub(super) fn render_layout_only(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        if self.surface.is_transcript() && self.surface.inline.transcript_reset_pending {
            return self.render_expanded_transcript(terminal);
        }

        #[cfg(feature = "devtools")]
        let total_start = Instant::now();

        #[cfg(feature = "profiling-tracing")]
        let _render_span = trace_span!("app.render_layout_only").entered();

        let size = terminal.size()?;

        let bounds = self.content_bounds(size.width, size.height);

        #[cfg(feature = "devtools")]
        self.install_devtools_overlay();

        if !self.dirty_component_scopes.is_empty()
            && !self
                .core
                .refresh_cached_scopes(&self.dirty_component_scopes, bounds)
        {
            crate::debug::internal_log!(
                "[tui-lipan] layout-only fallback: refresh_cached_scopes failed for {:?}",
                self.dirty_component_scopes
            );
            self.dirty_component_scopes.clear();
            self.dirty_scope_set.clear();
            return self.render(terminal);
        }
        self.dirty_component_scopes.clear();
        self.dirty_scope_set.clear();

        self.push_drag_layout_collapse_hint();
        let text_area_drag_snapshot = self.active_text_area_drag_snapshot();
        #[cfg(feature = "devtools")]
        let reconcile_start = Instant::now();
        let needs_full_render = (|| -> Result<bool> {
            for pass in 0..RENDER_STABILITY_MAX_PASSES {
                let scroll_generations = self.core.scroll.view_generations();
                let ok = self.core.reconcile_cached_element(
                    bounds,
                    self.focus.focused,
                    self.focus.focused_key.as_ref(),
                    self.mouse.hovered,
                );
                if !ok {
                    crate::debug::internal_log!(
                        "[tui-lipan] layout-only fallback: reconcile_cached_element failed"
                    );
                    return Ok(true);
                }

                let render_time_dirty = self.drain_render_time_messages()?;
                // Fall back to a full render only when a view() reads scroll
                // data that actually moved (full TextArea metrics vs. rare
                // scrollbar-visibility flips): cached views are stale then.
                // Otherwise the reconcile above already applied the change.
                if self
                    .core
                    .scroll
                    .view_dependencies_stale(&scroll_generations)
                {
                    crate::debug::internal_log!(
                        "[tui-lipan] layout-only fallback: scroll generation changed"
                    );
                    return Ok(true);
                }

                match render_time_dirty {
                    DirtyLevel::Full => {
                        crate::debug::internal_log!(
                            "[tui-lipan] layout-only fallback: render-time message was Full"
                        );
                        return Ok(true);
                    }
                    DirtyLevel::LayoutOnly => {
                        if !self.dirty_component_scopes.is_empty()
                            && !self
                                .core
                                .refresh_cached_scopes(&self.dirty_component_scopes, bounds)
                        {
                            crate::debug::internal_log!(
                                "[tui-lipan] layout-only fallback: render-time refresh_cached_scopes failed for {:?}",
                                self.dirty_component_scopes
                            );
                            return Ok(true);
                        }
                        self.dirty_component_scopes.clear();
                        self.dirty_scope_set.clear();
                    }
                    DirtyLevel::PaintOnly | DirtyLevel::None => return Ok(false),
                }

                if pass + 1 == RENDER_STABILITY_MAX_PASSES {
                    #[cfg(debug_assertions)]
                    crate::debug::internal_log!(
                        "[tui-lipan] layout render stability cap reached: render_time_dirty={:?}",
                        render_time_dirty,
                    );
                    return Ok(true);
                }
            }

            Ok(true)
        })();
        self.pop_drag_layout_collapse_hint();
        let needs_full_render = needs_full_render?;
        self.restore_active_text_area_drag_snapshot(text_area_drag_snapshot);
        if needs_full_render {
            return self.render(terminal);
        }

        // A layout-only frame can still change the content's natural height
        // (e.g. a list gaining rows). When the auto inline height moves, fall
        // back to a full render so the tree is reconciled at the new bounds.
        if self.sync_inline_auto_height(terminal, bounds)?.is_some() {
            crate::debug::internal_log!(
                "[tui-lipan] layout-only fallback: inline auto height moved"
            );
            return self.render(terminal);
        }
        #[cfg(feature = "devtools")]
        let reconcile_duration = reconcile_start.elapsed();

        let draw_duration = self.finalize_after_reconcile(terminal, DrawMode::LayoutOnly)?;
        #[cfg(feature = "devtools")]
        self.record_devtools_frame_metrics(
            DrawMode::LayoutOnly,
            total_start.elapsed(),
            reconcile_duration,
            draw_duration,
        );
        #[cfg(not(feature = "devtools"))]
        let _ = draw_duration;
        Ok(())
    }

    fn render_expanded_transcript(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        self.core.clear_pending_transcript_entries();

        let (width, screen_height) = crossterm::terminal::size()?;
        let initial_bounds = self.content_bounds(width, screen_height);

        self.push_drag_layout_collapse_hint();
        let initial_result = self.render_element_until_scroll_stable(initial_bounds);
        self.pop_drag_layout_collapse_hint();
        initial_result?;

        let natural_height = self
            .core
            .cached_expanded_element
            .as_ref()
            .map(|element| {
                min_size_constrained(element, Some(initial_bounds.w), None)
                    .1
                    .max(1)
            })
            .unwrap_or(1);
        let full_bounds = Rect {
            x: 0,
            y: 0,
            w: initial_bounds.w,
            h: natural_height,
        };
        if full_bounds != initial_bounds {
            self.push_drag_layout_collapse_hint();
            let full_result = self.render_element_until_scroll_stable(full_bounds);
            self.pop_drag_layout_collapse_hint();
            full_result?;
        }

        let document = self.core.transcript_replay_document(false);

        // Erase through the old terminal's backend first (ensures no buffered
        // output races with the raw escape sequences), then create the new
        // terminal.  This order matters: if create_inline_terminal fails the
        // erase has not yet run and the display is still intact.
        host_terminal_erase_scrollback_and_visible(terminal)?;
        *terminal = create_inline_terminal(natural_height)?;
        // Seed last_terminal_size with the new terminal's size so that the
        // size-change guard in draw_current_tree sees no change and skips the
        // ephemeral-mode clear path on the very next draw.
        if let Ok(size) = terminal.size() {
            self.surface.inline.last_terminal_size = (size.width, size.height);
        }
        self.last_frame_snapshot = None;
        self.scroll_diff_snapshot = None;
        self.last_scroll_frames.clear();
        self.dirty_component_scopes.clear();
        self.dirty_scope_set.clear();

        self.flush_transcript_document_entries(terminal, document)?;
        self.finalize_after_reconcile(terminal, DrawMode::Full)?;
        self.surface.inline.transcript_reset_pending = false;
        Ok(())
    }

    fn finalize_after_reconcile(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        draw_mode: DrawMode,
    ) -> Result<std::time::Duration> {
        #[cfg(feature = "image")]
        self.refresh_image_layout_suspension();
        if let ActiveDrag::Scrollbar(drag) = &self.drag.active {
            let mut drag = drag.clone();
            if !self.core.tree.is_valid(drag.id)
                && scrollbar::rebind_drag_to_key(&self.core.tree, &mut drag)
            {
                self.drag.active = ActiveDrag::Scrollbar(drag.clone());
            }

            if self.core.tree.is_valid(drag.id) {
                let rect = self.core.tree.node(drag.id).rect;
                if self.drag.scrollbar_rect != Some(rect) {
                    self.drag.scrollbar_rect = Some(rect);
                    self.drag.scrollbar_recalc = true;
                }
            } else {
                self.drag.clear();
            }
        }
        // Honor a focus request issued during this frame's expand/reconcile
        // (e.g. from a newly-mounted component's `init()`) before
        // `restore_focus` falls back to the first focusable node.
        self.apply_pending_focus_request();
        focus::restore_focus(
            &self.core.tree,
            &mut self.focus.focused,
            &mut self.focus.focused_key,
            &mut self.focus.focused_tag,
            self.focus.policy,
        );
        self.ensure_overlay_focus();
        self.notify_focus_change();
        #[cfg(feature = "devtools")]
        self.update_devtools_focus_metrics();
        #[cfg(feature = "terminal")]
        self.emit_terminal_focus_change();
        self.mouse.hovered = self.mouse.hovered.filter(|id| self.core.tree.is_valid(*id));
        self.refresh_active_selection_drag_from_last_pointer();
        self.refresh_hover_from_last_mouse();
        self.prune_widget_caches_if_needed();

        // Tree structure changed - rebuild the JoinIndex for frame adjacency lookups.
        self.cached_join_index =
            crate::backend::ratatui_backend::render::build_join_index(&self.core.tree);

        // Layout changed - invalidate the scrollbar metrics cache so that
        // thumb positions are recomputed against the new geometry.
        self.scrollbar_metrics_cache.borrow_mut().clear();

        if self.core.tree.has_spinners() {
            self.update_spinner_frames();
        }

        let draw_duration = self.draw_current_tree(terminal, draw_mode)?;
        self.sync_mouse_motion_capture(terminal)?;
        Ok(draw_duration)
    }

    /// Paint-only fast path: skip the full component-view → expand →
    /// reconcile → layout pipeline and just re-draw the existing tree.
    ///
    /// Used when the only change is a cursor blink toggle, spinner frame
    /// advance, or image frame advance.
    pub(super) fn render_paint_only(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        #[cfg(feature = "profiling-tracing")]
        let _render_span = trace_span!("app.render_paint_only").entered();

        #[cfg(debug_assertions)]
        self.debug_paint_verify_root_view_claim(terminal);

        #[cfg(feature = "devtools")]
        let total_start = Instant::now();

        if self.surface.is_transcript() && self.surface.inline.transcript_reset_pending {
            return self.render_expanded_transcript(terminal);
        }

        let draw_duration = self.draw_current_tree(terminal, DrawMode::PaintOnly)?;
        #[cfg(feature = "devtools")]
        self.record_devtools_frame_metrics(
            DrawMode::PaintOnly,
            total_start.elapsed(),
            std::time::Duration::ZERO,
            draw_duration,
        );
        #[cfg(not(feature = "devtools"))]
        let _ = draw_duration;
        Ok(())
    }

    #[cfg(debug_assertions)]
    /// Root `Update::paint()` only: compares last committed root `view()` snapshot to a fresh `view()`.
    /// Nested scopes returning `Update::paint()` are not checked (would need per-scope snapshots).
    fn debug_paint_verify_root_view_claim(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) {
        if !self.debug_paint_claim_root {
            return;
        }
        self.debug_paint_claim_root = false;
        let Some(prev) = self.core.debug_last_root_view_before_expand.as_ref() else {
            return;
        };
        let Ok(size) = terminal.size() else {
            return;
        };
        let bounds = self.content_bounds(size.width, size.height);
        self.core.ctx.set_viewport(bounds);
        self.core.ctx.set_active_theme(self.core.theme.clone());
        let current = self.core.component.view(&self.core.ctx);
        if !crate::core::element_debug::debug_element_tree_eq(prev, &current) {
            crate::debug::internal_log!(
                "[tui-lipan] Update::paint() at root but root view() output changed; use Update::layout() or Update::full() instead"
            );
        }
    }

    fn draw_current_tree(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
        draw_mode: DrawMode,
    ) -> Result<std::time::Duration> {
        #[cfg(feature = "profiling-tracing")]
        let _draw_span = trace_span!("app.draw_current_tree").entered();

        self.flush_inline_inserts(terminal)?;
        if !(draw_mode == DrawMode::PaintOnly && self.surface.is_inline()) {
            terminal.autoresize()?;
        }

        if self.surface.is_inline() {
            let size = terminal.size()?;
            let current_size = (size.width, size.height);
            let size_changed = current_size != self.surface.inline.last_terminal_size
                && self.surface.inline.last_terminal_size != (0, 0);
            // For InlineEphemeral, `is_transcript()` is false, so this branch
            // runs on resize.  `has_transcript_history()` is always false for
            // non-transcript modes (transcript APIs are no-ops outside
            // InlineTranscript) but is kept as a defensive guard.
            if size_changed && !self.surface.is_transcript() && !self.core.has_transcript_history()
            {
                let new_y = terminal.get_frame().area().y;
                let old_y = self.surface.inline.viewport_metrics.y;
                let clear_from = old_y.min(new_y);
                let _ = execute!(
                    terminal.backend_mut(),
                    MoveTo(0, clear_from),
                    Print("\x1b[J")
                );
                terminal.clear()?;
            }
            self.surface.inline.last_terminal_size = current_size;
        }

        self.sync_pointer_item_hover_suppression();

        let focused = self.focus.focused;
        let hovered = self.mouse.hovered;
        let blink_visible = self.animation.blink_visible;
        let tree = &self.core.tree;
        let mouse_pos = self.mouse.last_mouse;
        let frame_area = {
            let frame = terminal.get_frame();
            frame.area()
        };
        if let Some(title) = &self.title {
            let _ = execute!(terminal.backend_mut(), Print(format!("\x1b]2;{title}\x07")));
        }
        let _ = execute!(terminal.backend_mut(), Print("\x1b[?2026h"));
        #[cfg(any(feature = "devtools", feature = "profiling-tracing"))]
        let draw_start = Instant::now();
        let current_scroll_frames = capture_scroll_frames(tree);
        let scroll_plan =
            self.prepare_incremental_scroll_plan(draw_mode, frame_area, &current_scroll_frames);
        let cursor_position = StdCell::new(None);
        self.seed_dnd_snapshot_cells_from_last_frame_if_needed();
        let tab_drag_preview_label: Option<std::sync::Arc<str>> = match &self.drag.active {
            // Suppress text label when a snapshot preview is available.
            ActiveDrag::DraggableTabBar(d) if d.started && d.preview_snapshot_anchor.is_none() => {
                d.preview_label.clone()
            }
            _ => None,
        };
        let (mut drag_preview_label_owned, mut drag_preview_snapshot_rect) = self
            .drag_preview_context()
            .map_or((None, None), |(_, _, drag)| match &drag.preview {
                crate::widgets::DragPreview::Label(label) => (Some(label.clone()), None),
                crate::widgets::DragPreview::SourceSnapshot => {
                    let rect = drag
                        .preview_snapshot_anchor
                        .filter(|r| !r.is_empty() && r.x >= 0 && r.y >= 0)
                        .map(|r| ratatui::layout::Rect::new(r.x as u16, r.y as u16, r.w, r.h))
                        .or_else(|| {
                            if !self.core.tree.is_valid(drag.source_id) {
                                return None;
                            }
                            let r = self.core.tree.node(drag.source_id).rect;
                            (r.x >= 0 && r.y >= 0).then(|| {
                                ratatui::layout::Rect::new(r.x as u16, r.y as u16, r.w, r.h)
                            })
                        });
                    (None, rect)
                }
                crate::widgets::DragPreview::None => (None, None),
            });
        // For tab bar drags, derive snapshot rect from the anchor set at drag activation.
        if drag_preview_snapshot_rect.is_none()
            && let ActiveDrag::DraggableTabBar(d) = &self.drag.active
            && d.started
        {
            drag_preview_snapshot_rect = d
                .preview_snapshot_anchor
                .filter(|r| !r.is_empty() && r.x >= 0 && r.y >= 0)
                .map(|r| ratatui::layout::Rect::new(r.x as u16, r.y as u16, r.w, r.h));
        }
        if drag_preview_label_owned.is_none() {
            drag_preview_label_owned = tab_drag_preview_label;
        }
        let drag_preview_at_mouse = matches!(
            &self.drag.active,
            ActiveDrag::DragDrop(d) if d.started
        ) || matches!(
            &self.drag.active,
            ActiveDrag::DraggableTabBar(d) if d.started
        );
        let (drag_preview_max_width, drag_preview_max_height) = self
            .drag_preview_context()
            .and_then(|(_, _, drag)| {
                if !matches!(drag.preview, DragPreview::SourceSnapshot) {
                    return None;
                }
                if !self.core.tree.is_valid(drag.source_id) {
                    return None;
                }
                match &self.core.tree.node(drag.source_id).kind {
                    NodeKind::DragSource(s) => Some((s.preview_max_width, s.preview_max_height)),
                    _ => None,
                }
            })
            .unwrap_or((None, None));
        let drop_slot_source_preview_rect = self.drag_preview_context().and_then(|(_, _, drag)| {
            if !matches!(drag.preview, DragPreview::SourceSnapshot) {
                return None;
            }
            let target_id = drag.hovered_target?;
            if !self.core.tree.is_valid(target_id) {
                return None;
            }
            let node = self.core.tree.node(target_id);
            let NodeKind::DropTarget(target) = &node.kind else {
                return None;
            };
            if target.drop_slot != crate::widgets::DropSlot::SourcePreview {
                return None;
            }
            let r = node.rect;
            (r.x >= 0 && r.y >= 0)
                .then(|| ratatui::layout::Rect::new(r.x as u16, r.y as u16, r.w, r.h))
        });
        let drag_preview_label = drag_preview_label_owned.as_deref();
        if scroll_plan.is_some() && self.focused_node_requests_cursor() {
            cursor_position.set(self.incremental_cursor_position());
        }

        let ctx = RenderContext {
            tree,
            focused,
            hovered,
            mouse_pos,
            suppress_pointer_item_hover_nodes: Some(&self.mouse.suppress_pointer_item_hover_nodes),
            blink_visible,
            effect_phase: self.animation.effect_phase_tick,
            images_enabled: !self.surface.is_inline(),
            contrast_policy: self.contrast_policy,
            read_only_selection: Some(&self.widgets.read_only_selection),
            scrollbar_metrics_cache: &self.scrollbar_metrics_cache,
            overlay_bg_snapshot: &self.overlay_bg_snapshot,
            join_index: &self.cached_join_index,
            cursor_position: &cursor_position,
            terminal_bg: self
                .terminal_bg
                .map(crate::backend::ratatui_backend::common::to_ratatui_color),
            drag_preview_label,
            drag_preview_at_mouse,
            drag_preview_snapshot_rect,
            dnd_snapshot_cells: &self.dnd_snapshot_cells,
            drag_preview_max_width,
            drag_preview_max_height,
            drop_slot_source_preview_rect,
            paint_glyph_caches: Some(self.paint_glyph_caches.clone()),
            copy_feedback: Some(&self.copy_feedback),
            copy_feedback_style: self.clipboard_config.copy_feedback_style,
        };

        self.paint_glyph_caches.borrow_mut().clear();

        // Install the opt-in root viewport background for this draw. The scope
        // covers both the full-redraw and the incremental-scroll fast paths so
        // exposed scroll rows stay filled with the configured surface.
        let _screen_bg_scope =
            crate::backend::ratatui_backend::common::push_render_screen_background(
                self.resolved_screen_background(),
            );

        if let Some(plan) = scroll_plan.as_ref() {
            let previous_snapshot = self.last_frame_snapshot.as_ref().expect("snapshot exists");
            replace_buffer_snapshot(&mut self.scroll_diff_snapshot, previous_snapshot);

            let cursor_requested = self.focused_node_requests_cursor();
            if cursor_requested && cursor_position.get().is_none() {
                cursor_position.set(self.incremental_cursor_position());
            }
            let last_snapshot = self.last_frame_snapshot.as_mut().expect("snapshot exists");
            let diff_snapshot = self
                .scroll_diff_snapshot
                .as_mut()
                .expect("diff snapshot exists");
            Self::draw_incremental_scroll(
                terminal,
                &ctx,
                plan,
                last_snapshot,
                diff_snapshot,
                cursor_requested,
            )?;
        } else {
            let completed = terminal.draw(|f| {
                render(f, &ctx);
            })?;
            replace_buffer_snapshot(&mut self.last_frame_snapshot, completed.buffer);
        }
        #[cfg(any(feature = "devtools", feature = "profiling-tracing"))]
        let draw_duration = draw_start.elapsed();
        #[cfg(feature = "profiling-tracing")]
        tracing::trace!(
            target: "tui_lipan::perf",
            draw_ms = draw_duration.as_secs_f64() * 1000.0
        );
        let _ = execute!(terminal.backend_mut(), Print("\x1b[?2026l"));

        self.last_scroll_frames = current_scroll_frames;

        self.set_viewport_metrics(frame_area);
        self.capture_inline_cursor_offset(frame_area, cursor_position.get());
        self.stabilize_inline_resize_anchor(terminal)?;

        self.terminal.update_cursor(
            terminal.backend_mut(),
            &self.core.tree,
            self.focus.focused,
            &self.widgets.text_area_vim_state,
        )?;

        #[cfg(any(feature = "devtools", feature = "profiling-tracing"))]
        {
            Ok(draw_duration)
        }
        #[cfg(not(any(feature = "devtools", feature = "profiling-tracing")))]
        {
            Ok(std::time::Duration::ZERO)
        }
    }

    fn prune_widget_caches_if_needed(&mut self) {
        let epoch = self.core.tree.epoch();
        if self.last_post_reconcile_epoch == epoch {
            return;
        }
        self.last_post_reconcile_epoch = epoch;

        self.widgets
            .read_only_selection
            .retain(|id, _| self.core.tree.is_valid(*id));
        self.widgets.input_history.retain(|id, _| {
            self.core.tree.is_valid(*id)
                && matches!(self.core.tree.node(*id).kind, NodeKind::Input(_))
        });
        self.widgets.textarea_history.retain(|id, _| {
            self.core.tree.is_valid(*id)
                && matches!(self.core.tree.node(*id).kind, NodeKind::TextArea(_))
        });
        self.widgets.text_area_vim_state.retain(|id, _| {
            self.core.tree.is_valid(*id)
                && matches!(&self.core.tree.node(*id).kind, NodeKind::TextArea(ta) if ta.vim_motions)
        });
        self.widgets.hex_history.retain(|id, _| {
            self.core.tree.is_valid(*id)
                && matches!(self.core.tree.node(*id).kind, NodeKind::HexArea(_))
        });
        self.widgets.hex_pending_edit.retain(|id, _| {
            self.core.tree.is_valid(*id)
                && matches!(self.core.tree.node(*id).kind, NodeKind::HexArea(_))
        });
    }

    pub(super) fn focused_node_has_cursor_anchor(&self) -> bool {
        let Some(id) = self.focus.focused else {
            return false;
        };
        if !self.core.tree.is_valid(id) {
            return false;
        }

        match &self.core.tree.node(id).kind {
            NodeKind::Input(node) => !node.disabled && !node.read_only,
            NodeKind::TextArea(node) => !node.disabled && !node.read_only,
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(node) => {
                node.cursor_visible
                    && node.scrollback_offset == 0
                    && node
                        .selection
                        .as_ref()
                        .map(|selection| selection.is_empty())
                        .unwrap_or(true)
            }
            _ => false,
        }
    }

    pub(super) fn focused_node_requests_cursor(&self) -> bool {
        let Some(id) = self.focus.focused else {
            return false;
        };
        if !self.core.tree.is_valid(id) {
            return false;
        }

        // When the host terminal window is unfocused, keep the hardware
        // cursor visible regardless of blink phase so the emulator can
        // render its native unfocused glyph (outlined block). Hiding the
        // cursor during the blink-off half-cycle would leave nothing for
        // the emulator to outline.
        let blink_on = !self.focus.window_focused || self.animation.blink_visible;

        match &self.core.tree.node(id).kind {
            NodeKind::Input(node) => {
                !node.disabled
                    && !node.read_only
                    && blink_on
                    && node
                        .anchor
                        .map(|anchor| anchor == node.cursor)
                        .unwrap_or(true)
            }
            NodeKind::TextArea(node) => {
                !node.disabled
                    && !node.read_only
                    && blink_on
                    && node
                        .anchor
                        .map(|anchor| anchor == node.cursor)
                        .unwrap_or(true)
            }
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(node) => {
                node.cursor_visible
                    && blink_on
                    && node.scrollback_offset == 0
                    && node
                        .selection
                        .as_ref()
                        .map(|selection| selection.is_empty())
                        .unwrap_or(true)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    #[cfg(feature = "devtools")]
    use std::time::Duration;
    #[cfg(feature = "devtools")]
    use web_time::Instant;

    use crate::app::context::SurfaceMode;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Position;
    use ratatui::widgets::{Paragraph, Widget};
    use ratatui::{Terminal, TerminalOptions, Viewport};

    use super::inline::{
        clear_inline_transcript_surface, reanchor_inline_viewport_to_top,
        wrap_transcript_visual_lines,
    };
    use super::{AppRunner, DrawMode, capture_scroll_frames};
    use crate::app::App;
    use crate::app::interaction_state::{DirtyLevel, DirtyTracker};
    use crate::core::component::{Component, Context, Update};
    use crate::runtime::RuntimeCore;
    use crate::style::{Length, Rect, Style, Theme};
    use crate::widgets::{
        Frame, ScrollEvent, ScrollView, ScrollViewportEvent, Text, TextArea, VStack,
    };

    #[cfg(feature = "devtools")]
    use crate::app::context::DevToolsConfig;

    struct ScrollWithInput;

    struct ScrollWithHoverableFrame;

    struct ViewportChangeSiblingState;

    struct TextAreaRerenderOnScroll;

    enum ViewportChangeMsg {
        Viewport(ScrollViewportEvent),
    }

    #[derive(Clone, Debug)]
    enum TextAreaScrollMsg {
        Scroll(ScrollEvent),
    }

    impl Component for ScrollWithInput {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
            VStack::new()
                .height(Length::Px(10))
                .child(
                    ScrollView::new()
                        .offset(1)
                        .children((0..8).map(|i| Text::new(format!("row {i}")).into())),
                )
                .child(TextArea::new("hello").height(Length::Px(3)))
                .into()
        }
    }

    impl Component for ScrollWithHoverableFrame {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
            ScrollView::new()
                .offset(1)
                .children((0..8).map(|i| {
                    Frame::new()
                        .border(false)
                        .height(Length::Auto)
                        .padding((1, 0, 1, 2))
                        .hover_style(Style::new().dim_by(0.1))
                        .child(Text::new(format!("row {i}")))
                        .into()
                }))
                .into()
        }
    }

    impl Component for ViewportChangeSiblingState {
        type Message = ViewportChangeMsg;
        type Properties = ();
        type State = usize;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                ViewportChangeMsg::Viewport(event) => {
                    ctx.state = event.visible.len();
                }
            }
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> crate::core::element::Element {
            VStack::new()
                .child(Text::new(format!("visible:{}", ctx.state)))
                .child(
                    ScrollView::new()
                        .height(Length::Px(3))
                        .on_viewport_change(ctx.link().callback(ViewportChangeMsg::Viewport))
                        .children((0..6).map(|i| Text::new(format!("row {i}")).into())),
                )
                .into()
        }
    }

    impl Component for TextAreaRerenderOnScroll {
        type Message = TextAreaScrollMsg;
        type Properties = ();
        type State = usize;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                TextAreaScrollMsg::Scroll(event) => ctx.state = event.offset,
            }
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> crate::core::element::Element {
            let body = (0..24)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n");

            VStack::new()
                .child(Text::new(format!("offset:{}", ctx.state)).height(Length::Px(1)))
                .child(
                    TextArea::new(body)
                        .line_numbers(false)
                        .border(false)
                        .on_scroll(ctx.link().callback(TextAreaScrollMsg::Scroll)),
                )
                .into()
        }
    }

    fn make_runner() -> AppRunner<ScrollWithInput> {
        let app = App::new().mouse(false);
        AppRunner::new(app, ScrollWithInput, ())
    }

    fn make_hover_runner() -> AppRunner<ScrollWithHoverableFrame> {
        let app = App::new().mouse(false);
        AppRunner::new(app, ScrollWithHoverableFrame, ())
    }

    #[test]
    fn render_time_viewport_change_updates_sibling_in_same_render() {
        let app = App::new().mouse(false);
        let mut runner = AppRunner::new(app, ViewportChangeSiblingState, ());
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };

        runner.core = RuntimeCore::new_test(
            ViewportChangeSiblingState,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner
            .render_element_until_scroll_stable(viewport)
            .expect("render-time viewport message should process");

        let status = runner
            .core
            .tree
            .iter()
            .filter_map(|node| match &node.kind {
                crate::core::node::NodeKind::Text(text) => text.spans.first(),
                _ => None,
            })
            .map(|span| span.content.as_ref())
            .find(|content| content.starts_with("visible:"))
            .expect("status text should be rendered");

        assert_eq!(status, "visible:3");
    }

    #[test]
    fn incremental_cursor_position_tracks_focused_text_area_without_terminal_query() {
        let mut runner = make_runner();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };

        runner.core = RuntimeCore::new_test(
            ScrollWithInput,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner.core.render_element(viewport, None, None, None);

        let text_area_id = runner
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, crate::core::node::NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("text area exists");
        runner.focus.focused = Some(text_area_id);

        let cursor = runner
            .incremental_cursor_position()
            .expect("cursor position should be known from widget state");

        assert_eq!(cursor, Position::new(1, 8));
    }

    #[test]
    fn active_selection_drag_disables_incremental_scroll_plan() {
        let mut runner = make_runner();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };

        runner.core = RuntimeCore::new_test(
            ScrollWithInput,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner.core.render_element(viewport, None, None, None);

        runner.drag.active =
            crate::app::runner::ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
                id: runner
                    .core
                    .tree
                    .iter()
                    .find(|node| matches!(node.kind, crate::core::node::NodeKind::TextArea(_)))
                    .map(|node| node.id)
                    .expect("text area exists"),
                anchor: 0,
            });

        assert!(runner.active_selection_drag_requires_full_repaint());
        assert!(
            runner
                .prepare_incremental_scroll_plan(
                    DrawMode::LayoutOnly,
                    ratatui::layout::Rect::new(0, 0, viewport.w, viewport.h),
                    &[],
                )
                .is_none()
        );
    }

    #[test]
    fn changed_scroll_content_hash_disables_incremental_scroll_plan() {
        let mut runner = make_runner();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };

        runner.core = RuntimeCore::new_test(
            ScrollWithInput,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner.core.render_element(viewport, None, None, None);

        let current_frames = capture_scroll_frames(&runner.core.tree);
        let current = current_frames
            .first()
            .cloned()
            .expect("scroll view snapshot exists");
        assert!(current.content_hash.is_some());
        let previous = crate::app::runner::ScrollFrameSnapshot {
            scroll_offset: current.scroll_offset.saturating_sub(1),
            ..current.clone()
        };

        runner.last_scroll_frames = vec![previous.clone()];
        runner.last_frame_snapshot = Some(Buffer::empty(ratatui::layout::Rect::new(
            0, 0, viewport.w, viewport.h,
        )));

        assert!(
            runner
                .prepare_incremental_scroll_plan(
                    DrawMode::LayoutOnly,
                    ratatui::layout::Rect::new(0, 0, viewport.w, viewport.h),
                    &current_frames,
                )
                .is_some(),
            "unchanged content hash should allow the pure scroll fast path"
        );

        runner.last_scroll_frames = vec![crate::app::runner::ScrollFrameSnapshot {
            content_hash: previous.content_hash.map(|hash| hash.wrapping_add(1)),
            ..previous
        }];

        assert!(
            runner
                .prepare_incremental_scroll_plan(
                    DrawMode::LayoutOnly,
                    ratatui::layout::Rect::new(0, 0, viewport.w, viewport.h),
                    &current_frames,
                )
                .is_none(),
            "changed content hash must force a full draw so shifted rows are not stale"
        );
    }

    struct BothAxesScrollView;

    impl Component for BothAxesScrollView {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
            ScrollView::new()
                .axis(crate::widgets::ScrollAxis::Both)
                .h_scrollbar(true)
                .children((0..30).map(|_| {
                    Text::new("x".repeat(60))
                        .width(Length::Auto)
                        .height(Length::Px(1))
                        .into()
                }))
                .into()
        }
    }

    // Regression: the scroll-region fast path must not include the standalone
    // horizontal scrollbar row, or vertical scrolling shifts/duplicates the bar.
    #[test]
    fn capture_scroll_frames_excludes_horizontal_scrollbar_row() {
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        let mut runner = AppRunner::new(App::new().mouse(false), BothAxesScrollView, ());
        runner.core = RuntimeCore::new_test(
            BothAxesScrollView,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner.core.render_element(viewport, None, None, None);

        let frames = capture_scroll_frames(&runner.core.tree);
        let frame = frames.first().expect("scroll view snapshot exists");

        // 60-col content overflows the 40-col viewport, so the bottom row is the
        // horizontal scrollbar. The scrollable region must stop above it (rows 0..9).
        assert_eq!(frame.scroll_rows, 0..9);
    }

    #[test]
    fn active_text_area_drag_survives_full_rerenders_from_scroll_callbacks() {
        let app = App::new().mouse(false);
        let mut runner = AppRunner::new(app, TextAreaRerenderOnScroll, ());
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 5,
        };

        runner.core = RuntimeCore::new_test(
            TextAreaRerenderOnScroll,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner
            .render_element_until_scroll_stable(viewport)
            .expect("initial render should succeed");

        let text_area_id = runner
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, crate::core::node::NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("text area exists");
        let rect = runner.core.tree.node(text_area_id).rect;
        let x = rect.x.max(0) as u16;
        let y = rect.y.saturating_add(rect.h as i16).saturating_sub(1) as u16;

        runner.drag.active =
            crate::app::runner::ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
                id: text_area_id,
                anchor: 0,
            });
        runner.drag.last_pointer_pos = Some((x, y));
        if let crate::core::node::NodeKind::TextArea(node) =
            &mut runner.core.tree.node_mut(text_area_id).kind
        {
            node.cursor = 0;
            node.anchor = Some(0);
        }

        for _ in 0..5 {
            assert!(runner.tick_stationary_drag_autoscroll());

            let mut dirty = DirtyTracker::default();
            runner
                .process_pending_messages(&mut dirty)
                .expect("scroll callback should process");
            assert_eq!(dirty.level(), DirtyLevel::Full);

            runner
                .render_element_until_scroll_stable(viewport)
                .expect("drag render should preserve live text area state");
            runner.refresh_active_selection_drag_from_last_pointer();
        }

        let crate::core::node::NodeKind::TextArea(node) = &runner.core.tree.node(text_area_id).kind
        else {
            unreachable!()
        };
        assert!(
            node.scroll_offset >= 5,
            "scroll should keep advancing across drag-time rerenders"
        );
        assert!(node.cursor > 0);
        assert_eq!(node.anchor, Some(0));
    }

    #[test]
    fn hoverable_scroll_region_disables_incremental_scroll_plan_under_mouse() {
        let mut runner = make_hover_runner();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };

        runner.core = RuntimeCore::new_test(
            ScrollWithHoverableFrame,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner.core.render_element(viewport, None, None, None);

        let current_frames = capture_scroll_frames(&runner.core.tree);
        let current = current_frames
            .first()
            .cloned()
            .expect("scroll view snapshot exists");
        let previous = crate::app::runner::ScrollFrameSnapshot {
            scroll_offset: current.scroll_offset.saturating_sub(1),
            ..current.clone()
        };

        runner.last_scroll_frames = vec![previous];
        runner.last_frame_snapshot = Some(Buffer::empty(ratatui::layout::Rect::new(
            0, 0, viewport.w, viewport.h,
        )));
        runner.mouse.last_mouse = Some((1, current.scroll_rows.start));

        assert!(
            runner
                .prepare_incremental_scroll_plan(
                    DrawMode::LayoutOnly,
                    ratatui::layout::Rect::new(0, 0, viewport.w, viewport.h),
                    &current_frames,
                )
                .is_none()
        );
    }

    #[test]
    fn reanchor_inline_viewport_to_top_resets_stale_offset_after_resize() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(2),
            },
        )
        .expect("inline terminal should initialize");

        // Resize first so the viewport adopts the final terminal width. Under
        // ratatui 0.30 `resize` itself re-anchors the inline viewport to the top.
        terminal.backend_mut().resize(12, 5);
        terminal.autoresize().expect("autoresize should succeed");

        // Push history above the viewport so it sits at a non-top offset, the
        // state `reanchor_inline_viewport_to_top` is meant to reset.
        terminal
            .insert_before(3, |buf| {
                Paragraph::new("line 1\nline 2\nline 3").render(buf.area, buf);
            })
            .expect("history insert should succeed");
        assert!(terminal.get_frame().area().y > 0);

        reanchor_inline_viewport_to_top(&mut terminal)
            .expect("re-anchoring inline viewport should succeed");

        let area = terminal.get_frame().area();
        assert_eq!(area.y, 0);
        assert_eq!(area.height, 2);
        assert_eq!(area.width, 12);
    }

    #[test]
    fn clear_inline_transcript_surface_keeps_host_scrollback_and_reanchors_inline_viewport() {
        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(3),
            },
        )
        .expect("inline terminal should initialize");

        terminal
            .insert_before(2, |buf| {
                Paragraph::new("history-1\nhistory-2").render(buf.area, buf);
            })
            .expect("history insert should succeed");

        assert_eq!(terminal.get_frame().area().y, 2);

        clear_inline_transcript_surface(&mut terminal)
            .expect("transcript surface clear should reanchor viewport");

        let area = terminal.get_frame().area();
        assert_eq!(area.y, 0);
        assert_eq!(area.height, 3);
    }

    #[test]
    fn inline_element_commit_reuses_scratch_terminal_and_resizes_with_width() {
        let mut runner = make_runner();
        let backend = TestBackend::new(20, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(4),
            },
        )
        .expect("inline terminal should initialize");

        assert!(runner.inline_commit_scratch.is_none());
        let rendered = runner
            .render_inline_element_commit_buffer(19, Text::new("first line").into())
            .expect("first inline element commit should produce a buffer");
        assert_eq!(rendered.area.width, 19);

        let scratch = runner
            .inline_commit_scratch
            .as_mut()
            .expect("scratch terminal should be initialized");
        assert_eq!(scratch.current_buffer_mut().area.width, 19);

        terminal.backend_mut().resize(12, 8);
        terminal.autoresize().expect("autoresize should succeed");

        let rendered = runner
            .render_inline_element_commit_buffer(11, Text::new("second line").into())
            .expect("second inline element commit should produce a buffer");
        assert_eq!(rendered.area.width, 11);

        let scratch = runner
            .inline_commit_scratch
            .as_mut()
            .expect("scratch terminal should remain initialized");
        assert_eq!(scratch.current_buffer_mut().area.width, 11);
    }

    #[test]
    fn transcript_replay_wraps_visual_lines_for_narrow_widths() {
        let lines = [crate::style::RichText::from(vec![crate::style::Span::new(
            "alpha beta gamma",
        )])];

        let wrapped = wrap_transcript_visual_lines(&lines, 6);
        let joined = wrapped
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(joined.len() > 1);
        assert_eq!(joined.join(""), "alpha beta gamma");
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn hidden_devtools_panel_skips_frame_metrics_recording() {
        let mut runner = AppRunner::new(App::new().mouse(false), ScrollWithInput, ());
        assert!(!runner.devtools_state.borrow().visible);

        runner.record_devtools_frame_metrics(
            DrawMode::Full,
            Duration::from_millis(3),
            Duration::from_millis(1),
            Duration::from_millis(2),
        );

        assert!(runner.devtools_state.borrow().frame_history.is_empty());
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn disabled_metrics_config_skips_frame_history_even_when_visible() {
        let mut runner = AppRunner::new(
            App::new().mouse(false).devtools_config(DevToolsConfig {
                logs: true,
                metrics: false,
                show_framework_logs: true,
            }),
            ScrollWithInput,
            (),
        );
        runner.devtools_state.borrow_mut().set_visible(true);

        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        runner.core = RuntimeCore::new_test(
            ScrollWithInput,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner.core.render_element(viewport, None, None, None);

        runner.record_devtools_frame_metrics(
            DrawMode::LayoutOnly,
            Duration::from_millis(5),
            Duration::from_millis(2),
            Duration::from_millis(1),
        );

        assert!(runner.devtools_state.borrow().frame_history.is_empty());
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn visible_metrics_config_records_frame_history() {
        let mut runner = AppRunner::new(App::new().mouse(false), ScrollWithInput, ());
        runner.devtools_state.borrow_mut().set_visible(true);

        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        runner.core = RuntimeCore::new_test(
            ScrollWithInput,
            (),
            viewport,
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runner.core.init();
        runner.core.render_element(viewport, None, None, None);

        runner
            .devtools_state
            .borrow_mut()
            .push_frame_metrics(crate::devtools::FrameMetrics {
                timestamp: Instant::now(),
                dirty_level: "baseline".to_string(),
                total_duration: Duration::from_millis(1),
                reconcile_duration: Duration::from_millis(1),
                draw_duration: Duration::from_millis(1),
                node_count: 1,
                overlay_count: 0,
                memo_hits: 0,
                memo_misses: 0,
                memo_miss_reasons: Vec::new(),
                attributions: Vec::new(),
            });
        let baseline = runner.devtools_state.borrow().frame_history.len();

        runner.record_devtools_frame_metrics(
            DrawMode::PaintOnly,
            Duration::from_millis(7),
            Duration::ZERO,
            Duration::from_millis(4),
        );

        assert_eq!(
            runner.devtools_state.borrow().frame_history.len(),
            baseline + 1
        );
    }
}
