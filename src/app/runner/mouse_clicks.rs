//! Per-widget left-click handlers extracted from `dispatch_mouse`.
//!
//! Each method corresponds to a widget-specific branch that was previously
//! inlined in the monolithic `dispatch_mouse` function in `events.rs`.

use crate::app::input::mouse;
use crate::app::input::mouse::{
    CheckboxToggle, DocumentClick, DraggableTabBarAction, InputChange, ListSelect, ProgressChange,
    SliderChange, SplitterGrab, TableSelect, TabsChange, TextAreaChange,
};
use crate::app::input::scrollbar;
use crate::app::input::text_area_vim::{
    sync_visual_mode_for_external_selection, text_area_vim_search_feedback_for_text,
};
use crate::app::runner::ActiveDrag;
use crate::callback::Callback;
use crate::core::component::Component;
use crate::core::event::MouseEvent;
use crate::core::node::{NodeId, NodeKind, ScrollbarTarget};
use crate::style::ScrollbarVariant;
use crate::widgets::{InputEvent, ListEvent, TabsEvent};

#[cfg(feature = "terminal")]
use crate::utils::{GridPos, GridSelection};
#[cfg(feature = "terminal")]
use crate::widgets::internal::{terminal_mouse_content_rect, terminal_selection_text};

use super::AppRunner;

// Per-widget click handlers called by `dispatch_mouse`.
impl<C: Component> AppRunner<C> {
    pub(crate) fn sync_textarea_vim_external_selection(
        &mut self,
        params: crate::app::mouse_dispatch::TextareaVimExternalSelectionParams<'_>,
    ) {
        let crate::app::mouse_dispatch::TextareaVimExternalSelectionParams {
            id,
            vim_motions,
            read_only,
            has_on_change,
            on_vim_mode_change,
            cursor,
            anchor,
        } = params;
        if !vim_motions || read_only || !has_on_change {
            self.widgets.text_area_vim_state.remove(&id);
            if self.core.tree.is_valid(id)
                && let NodeKind::TextArea(node) = &mut self.core.tree.node_mut(id).kind
            {
                node.vim_mode = crate::widgets::TextAreaVimMode::Normal;
                node.vim_visual_line_caret = None;
                node.vim_search_feedback = None;
            }
            return;
        }

        let (mode, visual_line_caret) = {
            let state = self.widgets.text_area_vim_state.entry(id).or_default();
            if let Some(mode) = sync_visual_mode_for_external_selection(state, cursor, anchor)
                && let Some(cb) = on_vim_mode_change
            {
                cb.emit(mode);
            }
            (state.mode, state.visual_line_caret)
        };
        let search_feedback = if self.core.tree.is_valid(id) {
            self.widgets.text_area_vim_state.get(&id).and_then(|state| {
                let NodeKind::TextArea(node) = &self.core.tree.node(id).kind else {
                    return None;
                };
                text_area_vim_search_feedback_for_text(state, node.value.as_ref(), cursor)
            })
        } else {
            None
        };
        if self.core.tree.is_valid(id)
            && let NodeKind::TextArea(node) = &mut self.core.tree.node_mut(id).kind
        {
            node.vim_mode = mode;
            node.vim_visual_line_caret = visual_line_caret;
            node.vim_search_feedback = search_feedback;
        }
    }

    fn clear_shared_document_selection(&mut self, hit: NodeId) {
        let Some((shared_selection_id, scroll_view_id)) = (|| {
            let node = self.core.tree.node(hit);
            let NodeKind::DocumentView(doc) = &node.kind else {
                return None;
            };
            Some((
                doc.shared_selection_id.clone()?,
                self.nearest_ancestor_scroll_view(hit)?,
            ))
        })() else {
            return;
        };

        let ids: Vec<_> = self.core.tree.iter().map(|node| node.id).collect();
        for other_id in ids {
            if other_id == hit || !self.core.tree.is_valid(other_id) {
                continue;
            }

            let should_clear = {
                let node = self.core.tree.node(other_id);
                let NodeKind::DocumentView(other_doc) = &node.kind else {
                    continue;
                };
                crate::app::input::drag::shared_selection_id_matches(
                    other_doc.shared_selection_id.as_deref(),
                    shared_selection_id.as_ref(),
                ) && self.nearest_ancestor_scroll_view(other_id) == Some(scroll_view_id)
            };

            if should_clear
                && let NodeKind::DocumentView(other_doc_mut) =
                    &mut self.core.tree.node_mut(other_id).kind
            {
                other_doc_mut.selection_anchor = None;
                other_doc_mut.table_rect_selection = None;
            }
        }

        // Also clear offscreen stashed selections for this shared group
        // so scrolling a cleared selection back into view doesn't resurrect it.
        if self.core.tree.is_valid(scroll_view_id)
            && let NodeKind::ScrollView(sv) = &mut self.core.tree.node_mut(scroll_view_id).kind
        {
            sv.offscreen_doc_selections.clear();
        }
    }

    /// Handle left-click on a Slider widget.
    pub(crate) fn handle_slider_click(
        &mut self,
        _hit: NodeId,
        change: SliderChange,
        x: u16,
        y: u16,
    ) -> bool {
        self.drag.active =
            ActiveDrag::Slider(crate::app::input::drag::SliderDrag { id: change.node_id });
        // Perform immediate update for click
        if self.handle_slider_drag(x, y, change.node_id, true) {
            return true;
        }
        true
    }

    /// Handle left-click on a ProgressBar.
    pub(crate) fn handle_progress_click(&mut self, change: ProgressChange) -> bool {
        // Start drag tracking
        if change.draggable {
            self.drag.active =
                ActiveDrag::Progress(crate::app::input::drag::ProgressDrag { id: change.node_id });
        }
        if let Some(cb) = change.on_change {
            cb.emit(crate::widgets::ProgressEvent {
                progress: change.progress,
            });
            return true;
        }
        false
    }

    /// Handle left-click on a DraggableTabBar.
    pub(crate) fn handle_draggable_tab_bar_click(
        &mut self,
        action: DraggableTabBarAction,
        x: u16,
        dirty: bool,
    ) -> bool {
        if action.overflow_scroll_step != 0 {
            let rect = if self.core.tree.is_valid(action.node_id) {
                self.core.tree.node(action.node_id).rect
            } else {
                return dirty;
            };
            if self.core.tree.is_valid(action.node_id)
                && let NodeKind::DraggableTabBar(node_tabs) =
                    &mut self.core.tree.node_mut(action.node_id).kind
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
            if self.ensure_draggable_tab_bar_tab_visible(action.node_id, action.tab_index) {
                handled = true;
            }
            if action.tab_index != action.active
                && let Some(cb) = action.on_change
            {
                cb.emit(TabsEvent {
                    index: action.tab_index,
                });
                handled = true;
            }

            if action.draggable
                && self.core.tree.is_valid(action.node_id)
                && let NodeKind::DraggableTabBar(node_tabs) =
                    &self.core.tree.node(action.node_id).kind
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
                self.drag.active =
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

    /// Handle left-click on a Splitter handle.
    pub(crate) fn handle_splitter_click(&mut self, grab: SplitterGrab, x: u16, y: u16) -> bool {
        if !self.core.tree.is_valid(grab.node_id) {
            return false;
        }
        let (start_pos, start_sizes, orientation) = {
            let NodeKind::Splitter(node) = &mut self.core.tree.node_mut(grab.node_id).kind else {
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
            &self.core.tree,
            grab.node_id,
            orientation,
            x,
            y,
        );
        if let Some(sec) = &secondary
            && let NodeKind::Splitter(node) = &mut self.core.tree.node_mut(sec.id).kind
        {
            node.active_handle = Some(sec.handle);
        }
        self.drag.active = ActiveDrag::Splitter(crate::app::input::drag::SplitterDrag {
            id: grab.node_id,
            handle: grab.handle,
            start_pos,
            start_sizes,
            secondary,
        });
        true
    }

    /// Handle left-click on a List item.
    pub(crate) fn handle_list_click(
        &mut self,
        hit: NodeId,
        select: ListSelect,
        x: u16,
        y: u16,
    ) -> bool {
        if select.len == 0 {
            return false;
        }

        let mut inner = select.rect.inner(select.border, select.padding);
        if select.scrollbar {
            let use_integrated =
                select.border && matches!(select.scrollbar_variant, ScrollbarVariant::Integrated);
            let use_standalone = select.scrollbar && !use_integrated;
            if use_standalone && inner.w > 0 {
                inner.w = inner.w.saturating_sub(1);
            }
        }

        if inner.contains(x as i16, y as i16) {
            let row = (y as i32).saturating_sub(inner.y as i32) as usize;
            let visible = inner.h as usize;

            if let NodeKind::List(node) = &mut self.core.tree.node_mut(hit).kind {
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

                // Click top indicator: scroll up.
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

                // Click bottom indicator: scroll down.
                if has_bottom && row == visible.saturating_sub(1) {
                    let visible_items = crate::widgets::list::utils::visible_items_for_height(
                        &node.items,
                        node.offset,
                        inner.h,
                    );
                    let visible_for_scroll =
                        if select.show_scroll_indicators && total > visible_items {
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

                // Click item row.
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
                        let click_count =
                            mouse::click_count_at(&mut self.mouse.last_click, x, y, true);
                        let is_double = click_count == 2;
                        if let Some(cb) = &select.on_item_click {
                            cb.emit(ListEvent { index });
                        }
                        self.mouse.pointer_driven_item_hover_selection.insert(hit);
                        select.cb.emit(ListEvent { index });
                        if let Some(cb) = &select.on_activate
                            && (select.activate_on_click || is_double)
                        {
                            cb.emit(ListEvent { index });
                        }
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Handle left-click on a Table row.
    pub(crate) fn handle_table_click(
        &mut self,
        hit: NodeId,
        select: TableSelect,
        x: u16,
        y: u16,
    ) -> bool {
        if select.rows.is_empty() {
            return false;
        }

        let inner = select.rect.inner(select.border, select.padding);

        if inner.contains(x as i16, y as i16) {
            if select.show_scroll_indicators && select.top_indicator && (y as i16) == inner.y {
                if let NodeKind::Table(table_node) = &mut self.core.tree.node_mut(hit).kind {
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
                if let NodeKind::Table(table_node) = &mut self.core.tree.node_mut(hit).kind {
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

            if (y as i16) >= content_y {
                let rel_y = (y as i32).saturating_sub(content_y as i32) as u16;
                let found = crate::widgets::table::row_index_at_visual_offset(
                    &select.rows,
                    select.offset,
                    rel_y,
                    select.row_gap,
                );

                if let Some(index) = found {
                    if let NodeKind::Table(table_node) = &mut self.core.tree.node_mut(hit).kind {
                        table_node.scroll_override = Some(table_node.offset);
                    }
                    let click_count = mouse::click_count_at(&mut self.mouse.last_click, x, y, true);
                    let is_double = click_count == 2;
                    self.mouse.pointer_driven_item_hover_selection.insert(hit);
                    select.cb.emit(crate::widgets::TableEvent { index });
                    if is_double && let Some(cb) = &select.on_activate {
                        cb.emit(crate::widgets::TableEvent { index });
                    }
                    return true;
                }
            }
        }
        false
    }

    /// Handle left-click on a Terminal widget.
    #[cfg(feature = "terminal")]
    pub(crate) fn handle_terminal_click(
        &mut self,
        hit: NodeId,
        _mouse: MouseEvent,
        x: u16,
        y: u16,
        hover_dirty: bool,
    ) -> bool {
        if let NodeKind::Terminal(_) = &self.core.tree.node(hit).kind {
            let (inner, lines, on_selection, scrollback_offset) = {
                let node = self.core.tree.node(hit);
                let NodeKind::Terminal(term) = &node.kind else {
                    return hover_dirty;
                };

                let Some(content_rect) = terminal_mouse_content_rect(&self.core.tree, hit) else {
                    return hover_dirty;
                };

                (
                    content_rect,
                    term.lines.clone(),
                    term.on_selection.clone(),
                    term.scrollback_offset,
                )
            };

            if inner.contains(x as i16, y as i16) {
                let grid_col = (x as i16).saturating_sub(inner.x) as usize;
                let grid_row = (y as i16).saturating_sub(inner.y) as usize;
                let pos = GridPos {
                    row: grid_row,
                    col: grid_col,
                };

                // Get click count for double/triple click detection
                let click_count = mouse::click_count_at(&mut self.mouse.last_click, x, y, true);

                // Calculate the actual row in the scrollback buffer
                let _actual_row = scrollback_offset.saturating_add(grid_row);

                let (selection, anchor) = match click_count {
                    2 => {
                        // Double-click: select word (skip empty lines and whitespace)
                        let line_text: String = if let Some(line) = lines.get(grid_row) {
                            line.iter().map(|span| span.content.as_ref()).collect()
                        } else {
                            String::new()
                        };

                        // Skip selection for empty lines or whitespace-only lines
                        let trimmed = line_text.trim();
                        if trimmed.is_empty() {
                            // Treat as single click - no selection
                            (None, pos)
                        } else {
                            // Convert display column to byte position
                            let byte_pos = crate::utils::text::byte_at_col(&line_text, grid_col);
                            let (word_start_byte, word_end_byte) =
                                crate::app::input::text::word_at_byte(&line_text, byte_pos, None);

                            // Check if selected word is empty or whitespace-only
                            let word_text = &line_text[word_start_byte..word_end_byte];
                            if word_start_byte == word_end_byte || word_text.trim().is_empty() {
                                // No word at position or only whitespace - treat as single click
                                (None, pos)
                            } else {
                                // Convert byte positions back to display columns (using unicode width)
                                let word_start_col = unicode_width::UnicodeWidthStr::width(
                                    &line_text[..word_start_byte],
                                );
                                let word_end_col = unicode_width::UnicodeWidthStr::width(
                                    &line_text[..word_end_byte],
                                );

                                let anchor = GridPos {
                                    row: grid_row,
                                    col: word_start_col,
                                };
                                let cursor = GridPos {
                                    row: grid_row,
                                    col: word_end_col,
                                };
                                let mut sel = GridSelection::new(anchor);
                                sel.extend_to(cursor);
                                (Some(sel), anchor)
                            }
                        }
                    }
                    3 => {
                        // Triple-click: select text content of line (trimmed)
                        let line_text: String = if let Some(line) = lines.get(grid_row) {
                            line.iter().map(|span| span.content.as_ref()).collect()
                        } else {
                            String::new()
                        };

                        // Find the trimmed content boundaries
                        let trimmed = line_text.trim();
                        if trimmed.is_empty() {
                            // Empty line - no selection
                            (None, pos)
                        } else {
                            // Find byte positions of trimmed content
                            let leading_ws = line_text.len() - line_text.trim_start().len();
                            let trailing_ws = line_text.len() - line_text.trim_end().len();
                            let start_byte = leading_ws;
                            let end_byte = line_text.len() - trailing_ws;

                            // Convert to display columns
                            let start_col =
                                unicode_width::UnicodeWidthStr::width(&line_text[..start_byte]);
                            let end_col =
                                unicode_width::UnicodeWidthStr::width(&line_text[..end_byte]);

                            let anchor = GridPos {
                                row: grid_row,
                                col: start_col,
                            };
                            let cursor = GridPos {
                                row: grid_row,
                                col: end_col,
                            };
                            let mut sel = GridSelection::new(anchor);
                            sel.extend_to(cursor);
                            (Some(sel), anchor)
                        }
                    }
                    _ => {
                        // Single click: clear selection, position cursor
                        (None, pos)
                    }
                };

                if let NodeKind::Terminal(term) = &mut self.core.tree.node_mut(hit).kind {
                    term.selection = selection.clone();
                }
                if let Some(cb) = on_selection {
                    let text = selection
                        .as_ref()
                        .map(|sel| terminal_selection_text(&lines, sel));
                    cb.emit(crate::widgets::TerminalSelectionEvent { selection, text });
                }
                // Only start drag tracking if we have a selection (double/triple click)
                // or if the user starts dragging (handled in Drag events)
                self.drag.active =
                    ActiveDrag::Terminal(crate::app::input::drag::TerminalDrag { id: hit, anchor });
                return true;
            }
        }
        false
    }

    /// Handle left-click on a TextArea.
    pub(crate) fn handle_textarea_click(&mut self, change: TextAreaChange, x: u16, y: u16) -> bool {
        if change.on_change.is_none() && !change.read_only {
            return false;
        }

        let is_active = Some(change.node_id) == self.focus.focused || !change.focusable;
        let inner = change.rect.inner(change.border, change.padding);

        if is_active && inner.w > 0 && inner.h > 0 && change.rect.contains(x as i16, y as i16) {
            let (new_cursor, new_anchor, anchor_for_drag) =
                mouse::process_textarea_click(&change, x, y, &mut self.mouse.last_click);

            // Start drag tracking for selection
            self.drag.last_pointer_pos = None;
            self.drag.last_autoscroll_tick = None;
            self.drag.autoscroll_layout_dirty = false;
            self.drag.active = ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
                id: change.node_id,
                anchor: anchor_for_drag,
            });

            // Apply the visual selection immediately. Emit on_change on the
            // initial click so callers binding the cursor through state see
            // it persist even if the host rerenders between Down and Up
            // (the WASM dispatcher does this). Subsequent drag updates stay
            // on the paint-only path.
            if let NodeKind::TextArea(node) = &mut self.core.tree.node_mut(change.node_id).kind {
                node.cursor = new_cursor;
                node.anchor = new_anchor;
            }

            self.sync_textarea_vim_external_selection(
                crate::app::mouse_dispatch::TextareaVimExternalSelectionParams {
                    id: change.node_id,
                    vim_motions: change.vim_motions,
                    read_only: change.read_only,
                    has_on_change: change.on_change.is_some(),
                    on_vim_mode_change: change.on_vim_mode_change.as_ref(),
                    cursor: new_cursor,
                    anchor: new_anchor,
                },
            );

            if let Some(cb) = change.on_change.as_ref() {
                cb.emit(crate::widgets::TextAreaEvent {
                    value: change.value.clone(),
                    cursor: new_cursor,
                    anchor: new_anchor,
                });
                if let Some(state_cb) = change.on_editor_state_change.as_ref() {
                    let reason = if new_anchor != change.anchor {
                        crate::widgets::TextAreaStateChangeReason::SelectionChange
                    } else {
                        crate::widgets::TextAreaStateChangeReason::CursorMove
                    };
                    state_cb.emit(crate::widgets::TextAreaStateChangeEvent {
                        reason,
                        value: change.value.clone(),
                        cursor: new_cursor,
                        anchor: new_anchor,
                        edit: None,
                        vim_mode: None,
                    });
                }
            } else if change.read_only {
                self.widgets
                    .read_only_selection
                    .insert(change.node_id, (new_cursor, new_anchor));
            }

            self.animation.reset_blink();
            return true;
        }
        false
    }

    /// Handle left-click on a DocumentView.
    ///
    /// Returns `(handled, set_dirty)` because some branches set `dirty = true`
    /// without returning early.
    pub(crate) fn handle_document_view_click(
        &mut self,
        hit: NodeId,
        _mouse: MouseEvent,
        x: u16,
        y: u16,
    ) -> (bool, bool) {
        if !self.core.tree.is_valid(hit) {
            return (false, false);
        }
        let is_active = Some(hit) == self.focus.focused
            || matches!(&self.core.tree.node(hit).kind, NodeKind::DocumentView(doc) if !doc.focusable);
        if !is_active {
            return (false, false);
        }
        let NodeKind::DocumentView(doc) = &self.core.tree.node(hit).kind else {
            return (false, false);
        };

        let rect = self.core.tree.node(hit).rect;
        let inner = rect.inner(doc.border, doc.padding);
        let cl = doc.content_layout(inner);

        let content_rect = crate::style::Rect {
            x: cl.content_x,
            y: cl.content_y,
            w: cl.content_width,
            h: cl.content_height,
        };

        if content_rect.w > 0
            && content_rect.h > 0
            && content_rect.contains(x as i16, y as i16)
            && let Some(cursor) = crate::app::input::drag::document_view_cursor_from_coords(
                &self.core.tree,
                x,
                y,
                hit,
            )
        {
            let cursor = cursor.min(doc.visual_cache.flat_text.len());
            let click_count = mouse::click_count_at(&mut self.mouse.last_click, x, y, true);
            let click_count = if doc.multi_click_select {
                click_count
            } else {
                click_count.min(1)
            };
            let had_selection =
                doc.selection_anchor.is_some() || doc.table_rect_selection.is_some();
            if click_count > 1 || (click_count == 1 && had_selection) {
                self.mouse.click_consumed = true;
            }

            let table_hit = crate::app::input::drag::document_view_table_cell_from_coords(
                &self.core.tree,
                x,
                y,
                hit,
            );

            let drag_shared_id = doc.shared_selection_id.clone();
            let drag_sv_id = self.nearest_ancestor_scroll_view(hit);

            if click_count == 1
                && let Some(hit_cell) = table_hit
            {
                self.drag.active =
                    ActiveDrag::DocumentView(crate::app::input::drag::DocumentViewDrag {
                        id: hit,
                        anchor: crate::app::input::drag::DocumentViewDragAnchor::TableCell {
                            table_id: hit_cell.table_id,
                            row_index: hit_cell.row_index,
                            col_index: hit_cell.col_index,
                            row_line_index: hit_cell.row_line_index,
                            cell_line_anchor_byte: hit_cell.cell_line_anchor_byte,
                        },
                        shared_selection_id: drag_shared_id.clone(),
                        scroll_view_id: drag_sv_id,
                        shared_drag_anchor: None,
                    });
                if let NodeKind::DocumentView(doc_mut) = &mut self.core.tree.node_mut(hit).kind {
                    doc_mut.selection_cursor = cursor;
                    doc_mut.selection_anchor = None;
                    doc_mut.table_rect_selection = None;
                }
                self.clear_shared_document_selection(hit);
                self.animation.reset_blink();
                return (false, true);
            } else {
                let rel_y = (y as i16).saturating_sub(inner.y) as usize;
                let visual_idx = doc.scroll_offset.saturating_add(rel_y);

                let (new_cursor, new_anchor) = match click_count {
                    2 => {
                        if let (Some(&line_start), Some(line_text)) = (
                            doc.visual_cache.line_starts.get(visual_idx),
                            doc.visual_cache.line_texts.get(visual_idx),
                        ) {
                            let local_byte = cursor.saturating_sub(line_start).min(line_text.len());
                            let (word_start, word_end) = crate::app::input::text::word_at_byte(
                                line_text.as_ref(),
                                local_byte,
                                None,
                            );
                            (
                                line_start.saturating_add(word_end),
                                Some(line_start.saturating_add(word_start)),
                            )
                        } else {
                            (cursor, None)
                        }
                    }
                    3 => {
                        if let (Some(hit_cell), Some(&line_start)) =
                            (table_hit, doc.visual_cache.line_starts.get(visual_idx))
                        {
                            (
                                line_start.saturating_add(hit_cell.cell_text_end_byte),
                                Some(line_start.saturating_add(hit_cell.cell_text_start_byte)),
                            )
                        } else {
                            match doc.triple_click_mode {
                                crate::widgets::TripleClickSelectionMode::Line => {
                                    if let (Some(&line_start), Some(&line_len)) = (
                                        doc.visual_cache.line_starts.get(visual_idx),
                                        doc.visual_cache.line_lengths.get(visual_idx),
                                    ) {
                                        (line_start.saturating_add(line_len), Some(line_start))
                                    } else {
                                        (cursor, None)
                                    }
                                }
                                crate::widgets::TripleClickSelectionMode::Paragraph => {
                                    if let Some((start_idx, end_idx)) =
                                        crate::app::input::text::paragraph_line_range(
                                            &doc.visual_cache.line_texts,
                                            visual_idx,
                                        )
                                    {
                                        if let (
                                            Some(&line_start),
                                            Some(&line_end),
                                            Some(&line_len),
                                        ) = (
                                            doc.visual_cache.line_starts.get(start_idx),
                                            doc.visual_cache.line_starts.get(end_idx),
                                            doc.visual_cache.line_lengths.get(end_idx),
                                        ) {
                                            (line_end.saturating_add(line_len), Some(line_start))
                                        } else {
                                            (cursor, None)
                                        }
                                    } else {
                                        (cursor, None)
                                    }
                                }
                            }
                        }
                    }
                    _ => (cursor, None),
                };

                let anchor_for_drag = new_anchor.unwrap_or(new_cursor);
                let shared_drag_anchor = if let (Some(sid), Some(sv)) =
                    (drag_shared_id.as_ref(), drag_sv_id)
                    && crate::app::input::drag::scroll_view_has_multiple_shared_linear_docs(
                        &self.core.tree,
                        sv,
                        sid.as_ref(),
                    ) {
                    crate::app::input::drag::shared_document_drag_anchor_for_hit(
                        &self.core.tree,
                        sv,
                        hit,
                        anchor_for_drag,
                    )
                } else {
                    None
                };
                self.drag.active =
                    ActiveDrag::DocumentView(crate::app::input::drag::DocumentViewDrag {
                        id: hit,
                        anchor: crate::app::input::drag::DocumentViewDragAnchor::Linear(
                            anchor_for_drag,
                        ),
                        shared_selection_id: drag_shared_id,
                        scroll_view_id: drag_sv_id,
                        shared_drag_anchor,
                    });
                if let NodeKind::DocumentView(doc_mut) = &mut self.core.tree.node_mut(hit).kind {
                    doc_mut.selection_cursor = new_cursor;
                    doc_mut.selection_anchor = new_anchor;
                    doc_mut.table_rect_selection = None;
                }
                self.clear_shared_document_selection(hit);

                self.animation.reset_blink();
                return (false, true);
            }
        }
        (false, false)
    }

    /// Handle left-click on an Input field.
    pub(crate) fn handle_input_click(&mut self, change: InputChange, x: u16) -> bool {
        let is_active = Some(change.node_id) == self.focus.focused || !change.focusable;
        let inner = change.rect.inner(change.border, change.padding);

        if is_active && inner.w > 0 && change.rect.contains(x as i16, change.rect.y) {
            let (new_cursor, new_anchor, anchor_for_drag) =
                mouse::process_input_click(&change, x, &mut self.mouse.last_click);

            // Start drag tracking for selection
            self.drag.active = ActiveDrag::Input(crate::app::input::drag::InputDrag {
                id: change.node_id,
                anchor: anchor_for_drag,
            });

            if let Some(cb) = &change.on_change {
                cb.emit(InputEvent {
                    value: change.value,
                    cursor: new_cursor,
                    anchor: new_anchor,
                });
            } else if change.read_only {
                self.widgets
                    .read_only_selection
                    .insert(change.node_id, (new_cursor, new_anchor));
            }

            self.animation.reset_blink();
            return true;
        }
        false
    }

    /// Handle left-click on a HexArea.
    pub(crate) fn handle_hex_area_click(
        &mut self,
        hit: NodeId,
        mouse: MouseEvent,
        x: u16,
        y: u16,
    ) -> bool {
        if let NodeKind::HexArea(hex) = &self.core.tree.node(hit).kind
            && !hex.disabled
            && Some(hit) == self.focus.focused
            && let Some(hit_info) = crate::widgets::pointer_hit(
                self.core.tree.node(hit).rect,
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
            self.drag.active = ActiveDrag::HexArea(crate::app::input::drag::HexAreaDrag {
                id: hit,
                anchor: anchor_for_drag,
            });
            self.widgets.hex_pending_edit.remove(&hit);
            if let NodeKind::HexArea(hex_node) = &mut self.core.tree.node_mut(hit).kind {
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

            self.animation.reset_blink();
            return true;
        }
        false
    }

    // ------------------------------------------------------------------
    // Phase 1: tiny emit-only branches
    // ------------------------------------------------------------------

    /// Handle a tabs click (used for both `Tabs` and border-tabs).
    ///
    /// Returns `true` when the tab actually changed and the event was emitted.
    pub(crate) fn handle_tabs_click(&self, change: TabsChange) -> bool {
        if change.next != change.active {
            change.cb.emit(TabsEvent { index: change.next });
            return true;
        }
        false
    }

    /// Handle a checkbox toggle click.
    pub(crate) fn handle_checkbox_click(&self, toggle: CheckboxToggle) -> bool {
        toggle.cb.emit(crate::widgets::CheckboxEvent {
            state: toggle.state.toggle(),
        });
        true
    }

    /// Handle a document-click event (link / source-line click inside a
    /// `DocumentView`).
    pub(crate) fn handle_document_click_event(&self, click: DocumentClick) -> bool {
        click.cb.emit(crate::widgets::DocumentClickEvent {
            source_line: click.source_line,
            link: click.link,
        });
        true
    }

    /// Handle a graph-node click event.
    pub(crate) fn handle_graph_node_click(
        &mut self,
        click: crate::app::input::mouse::GraphNodeClick,
    ) -> bool {
        let focus_cb = {
            let NodeKind::Graph(graph) = &mut self.core.tree.node_mut(click.node_id).kind else {
                return false;
            };
            graph
                .set_focused_path(click.event.path.clone())
                .then(|| graph.on_node_focus.clone())
                .flatten()
        };

        let focus_changed = focus_cb.is_some();
        if let Some(cb) = focus_cb {
            cb.emit(click.event.clone());
        }
        if let Some(cb) = click.cb {
            cb.emit(click.event);
            true
        } else {
            focus_changed
        }
    }

    /// Handle a sequence-diagram item click event.
    pub(crate) fn handle_sequence_item_click(
        &self,
        click: crate::app::input::mouse::SequenceItemClick,
    ) -> bool {
        click.cb.emit(click.event);
        true
    }

    /// Handle a flowchart item click event.
    pub(crate) fn handle_flowchart_item_click(
        &self,
        click: crate::app::input::mouse::FlowchartItemClick,
    ) -> bool {
        match click {
            crate::app::input::mouse::FlowchartItemClick::Node { cb, event } => cb.emit(event),
            crate::app::input::mouse::FlowchartItemClick::Edge { cb, event } => cb.emit(event),
            crate::app::input::mouse::FlowchartItemClick::Subgraph { cb, event } => {
                cb.emit(event);
            }
        }
        true
    }

    /// Handle the fallback `on_click` callback attached to a node.
    pub(crate) fn handle_fallback_on_click(
        &self,
        cb: Callback<MouseEvent>,
        mouse: MouseEvent,
    ) -> bool {
        cb.emit(mouse);
        true
    }

    // ------------------------------------------------------------------
    // Phase 2: right-click textarea path
    // ------------------------------------------------------------------

    /// Handle a right-click landing on a `TextArea` node.
    ///
    /// Returns `true` when the click was consumed (the textarea's `on_click`
    /// callback was emitted).
    pub(crate) fn handle_right_click_textarea(&self, hit: NodeId, mouse: MouseEvent) -> bool {
        if let NodeKind::TextArea(text_area) = &self.core.tree.node(hit).kind
            && !text_area.disabled
            && let Some(cb) = &text_area.on_click
        {
            cb.emit(mouse);
            return true;
        }
        false
    }

    // ------------------------------------------------------------------
    // Phase 3: scrollbar press / start-drag
    // ------------------------------------------------------------------

    /// Handle a left-click landing on a scrollbar hit-zone.
    ///
    /// Initiates a scrollbar drag and performs an initial thumb snap.
    /// Returns `true` when the click was consumed, `false` when the node
    /// exposes scrollbar hit-zones but isn't actually scrollable (the caller
    /// should fall through to regular hit-testing).
    pub(crate) fn handle_scrollbar_click(
        &mut self,
        target: ScrollbarTarget,
        x: u16,
        y: u16,
    ) -> bool {
        let drag = {
            let node = self.core.tree.node(target.id);
            scrollbar::start_drag(node, target.axis, x, y)
        };
        if let Some(drag) = drag {
            let _ = self.focus_for_node(target.id);
            self.drag.active = ActiveDrag::Scrollbar(drag.clone());
            self.drag.scrollbar_rect = Some(self.core.tree.node(drag.id).rect);
            let handled = scrollbar::handle_drag(
                self.core.tree.node_mut(drag.id),
                drag.axis,
                x,
                y,
                drag.grab_offset,
                drag.grab_subcell,
            );
            if handled {
                scrollbar::remember_scroll_view_input_offset(&mut self.core.tree, drag.id);
            }
            return true;
        }
        // If the node exposes scrollbar hit-zones but isn't actually scrollable (e.g. no
        // overflow), don't consume the click. Fall through to regular hit-testing.
        false
    }

    // ------------------------------------------------------------------
    // Phase 4: drag-release finalization
    // ------------------------------------------------------------------

    /// Finalize an active drag operation on mouse button release.
    ///
    /// Returns `Some(true)` when a drag was active and cleanup was performed,
    /// `None` when no drag was active (`ActiveDrag::None`) so the caller can
    /// continue with other mouse-up handling.
    pub(crate) fn handle_drag_release(&mut self, x: u16, y: u16) -> Option<bool> {
        match &self.drag.active {
            ActiveDrag::Slider(_) => {
                self.drag.clear();
                Some(true)
            }
            ActiveDrag::DraggableTabBar(drag) => {
                let drag = drag.clone();
                let was_started = drag.started;
                self.clear_dnd_snapshot_cache();
                self.drag.clear();
                let handled = self.finish_draggable_tab_bar_drag(drag);
                let hover_dirty = self.update_hover(x, y);
                // Always repaint when a visible drag was in progress so the
                // floating preview is cleared even if no reorder occurred.
                Some(handled || hover_dirty || was_started)
            }
            ActiveDrag::Progress(_) => {
                self.drag.clear();
                Some(true)
            }
            ActiveDrag::DragDrop(drag) => {
                let payload = drag.payload.clone();
                let source_id = drag.source_id;
                let hovered_target = drag.hovered_target;
                let on_cancel = drag.on_cancel.clone();

                if let Some(target_id) = hovered_target {
                    if self.core.tree.is_valid(target_id) {
                        let rect = self.core.tree.node(target_id).rect;
                        let top = rect.y.max(0) as u16;
                        let local_y = y.saturating_sub(top);
                        if let NodeKind::DropTarget(target) =
                            &mut self.core.tree.node_mut(target_id).kind
                        {
                            target.dnd_highlighted = false;
                            if let Some(cb) = &target.on_drop {
                                cb.emit(crate::widgets::DropEvent {
                                    x,
                                    y,
                                    local_y,
                                    local_height: rect.h,
                                    payload: payload.clone(),
                                });
                            }
                            if let Some(cb) = &target.on_drag_leave {
                                cb.emit(crate::widgets::DragLeaveEvent {
                                    payload: payload.clone(),
                                });
                            }
                        } else if let Some(cb) = on_cancel {
                            cb.emit(crate::widgets::DragCancelEvent { payload });
                        }
                    } else if let Some(cb) = on_cancel {
                        cb.emit(crate::widgets::DragCancelEvent { payload });
                    }
                } else if let Some(cb) = on_cancel {
                    cb.emit(crate::widgets::DragCancelEvent { payload });
                }

                if self.core.tree.is_valid(source_id)
                    && let NodeKind::DragSource(source) =
                        &mut self.core.tree.node_mut(source_id).kind
                {
                    source.is_dragging = false;
                }
                self.clear_dnd_snapshot_cache();
                self.drag.clear();
                let _ = self.update_hover(x, y);
                Some(true)
            }
            ActiveDrag::Splitter(drag) => {
                let mut ids = vec![drag.id];
                if let Some(sec) = &drag.secondary {
                    ids.push(sec.id);
                }
                self.drag.clear();
                let mut resizes = Vec::new();
                for id in ids {
                    if !self.core.tree.is_valid(id) {
                        continue;
                    }
                    if let NodeKind::Splitter(node) = &mut self.core.tree.node_mut(id).kind {
                        node.active_handle = None;
                        if let Some(cb) = node.on_resize.clone() {
                            resizes.push((cb, node.split_id.clone(), node.weights.clone()));
                        }
                    }
                }
                for (cb, split_id, weights) in resizes {
                    cb.emit(crate::widgets::SplitterResizeEvent { split_id, weights });
                }
                Some(true)
            }
            ActiveDrag::Scrollbar(_) => {
                self.drag.clear();
                Some(true)
            }
            ActiveDrag::TextArea(_) => {
                if let ActiveDrag::TextArea(drag) = self.drag.active.clone() {
                    self.finish_textarea_drag(drag.id);
                }
                self.drag.clear();
                Some(true)
            }
            ActiveDrag::DocumentView(_) => {
                self.drag.clear();
                Some(true)
            }
            ActiveDrag::Input(_) => {
                self.drag.clear();
                Some(true)
            }
            ActiveDrag::HexArea(_) => {
                self.drag.clear();
                Some(true)
            }
            #[cfg(feature = "terminal")]
            ActiveDrag::Terminal(_) => {
                self.drag.clear();
                Some(true)
            }
            ActiveDrag::None => None,
        }
    }
}
