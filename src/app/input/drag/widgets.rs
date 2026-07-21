//! Drag handling for sliders, progress bars, text areas, and inputs.

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::ScrollbarVariant;
use crate::widgets::document_view::node::VisualLineKind;
use crate::widgets::{
    DragReorderMode, DraggableTabBar, DraggableTabReorderEvent, DraggableTabTransferEvent,
    InputEvent, ProgressEvent,
};

use super::*;
#[allow(clippy::type_complexity)]
pub(crate) fn handle_slider_drag(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
    require_track_y: bool,
) -> Option<(f64, Option<Callback<f64>>, Option<Callback<f64>>)> {
    if !tree.is_valid(id) {
        return None;
    }

    let node = tree.node(id);
    match &node.kind {
        NodeKind::Slider(slider) => {
            let track = crate::app::input::geometry::slider_track_geometry(slider, node.rect)?;

            if require_track_y && y as i16 != track.track_y {
                return None;
            }

            let rel_x = (x as i32).saturating_sub(track.track_x as i32);
            let track_len = track.track_w.saturating_sub(1);

            let fraction = if track_len == 0 {
                0.0
            } else if rel_x >= track.track_w as i32 {
                1.0
            } else {
                (rel_x as f64 / track_len as f64).clamp(0.0, 1.0)
            };

            let raw_value = slider.min + fraction * (slider.max - slider.min);
            let steps = ((raw_value - slider.min) / slider.step).round();
            let new_value = (slider.min + steps * slider.step).clamp(slider.min, slider.max);

            Some((new_value, slider.on_change.clone(), slider.on_click.clone()))
        }
        _ => None,
    }
}

/// Handle progress bar drag at the given x position.
pub(crate) fn handle_progress_drag(
    tree: &NodeTree,
    x: u16,
    id: NodeId,
) -> Option<(f64, Option<Callback<ProgressEvent>>)> {
    if !tree.is_valid(id) {
        return None;
    }

    let node = tree.node(id);
    match &node.kind {
        NodeKind::ProgressBar(progress) => {
            let p_value = crate::app::input::geometry::progress_value_at_x(progress, node.rect, x)?;
            Some((p_value, progress.on_change.clone()))
        }
        _ => None,
    }
}

/// Handle draggable tab bar drag updates.
pub(crate) fn handle_draggable_tab_bar_drag(
    tree: &NodeTree,
    x: u16,
    y: u16,
    mut drag: DraggableTabBarDrag,
) -> Option<(DraggableTabBarDrag, Option<DraggableTabDragEvent>)> {
    if !tree.is_valid(drag.id) {
        return None;
    }

    let node = tree.node(drag.id);
    let NodeKind::DraggableTabBar(bar) = &node.kind else {
        return None;
    };

    if bar.tabs.is_empty()
        || !bar
            .tabs
            .get(drag.current_index)
            .is_some_and(crate::widgets::draggable_tab_bar::is_reorderable_tab)
    {
        return Some((drag, None));
    }

    let dx = x.abs_diff(drag.start_x);
    if !drag.started && dx < drag.threshold {
        return Some((drag, None));
    }
    if !drag.started {
        drag.preview_snapshot_anchor =
            compute_tab_snapshot_anchor(tree, drag.source_id, drag.source_index);
    }
    drag.started = true;

    let target_id = tree
        .hit_test(x as i16, y as i16)
        .filter(|id| tree.is_valid(*id))
        .and_then(|id| match &tree.node(id).kind {
            NodeKind::DraggableTabBar(_) => Some(id),
            _ => None,
        })
        .unwrap_or(drag.id);

    if !tree.is_valid(target_id) {
        return Some((drag, None));
    }

    let target_node = tree.node(target_id);
    let NodeKind::DraggableTabBar(target_bar) = &target_node.kind else {
        return Some((drag, None));
    };
    if target_bar.tabs.is_empty() {
        return Some((drag, None));
    }

    let target_inner = target_node
        .rect
        .inner(target_bar.border, target_bar.padding);
    if target_inner.w == 0 || target_inner.h == 0 {
        return Some((drag, None));
    }

    let y_dist = ((y as i16) - target_inner.y).unsigned_abs();
    let y_tolerance = target_inner.h.max(1) + 2;
    if y_dist >= y_tolerance {
        return Some((drag, None));
    }

    let max_col = target_inner.w.saturating_sub(1);
    let rel = (x as i32).saturating_sub(target_inner.x as i32);
    let view_col = rel.clamp(0, max_col as i32) as usize;
    let target_disp_opts = target_bar.display_options();
    let target_vp_opts = target_bar.viewport_options(target_inner.w as usize);
    if DraggableTabBar::global_col_from_view_col(
        &target_bar.tabs,
        &target_disp_opts,
        &target_vp_opts,
        view_col,
    )
    .is_none()
    {
        return Some((drag, None));
    }

    let can_transfer = target_id != drag.id
        && drag.on_transfer.is_some()
        && drag.drag_group.is_some()
        && drag.drag_group == target_bar.drag_group
        && drag.bar_id.is_some()
        && target_bar.bar_id.is_some();

    if can_transfer {
        let to_index = DraggableTabBar::reorder_index_at_view_col(
            &target_bar.tabs,
            &target_disp_opts,
            &target_vp_opts,
            view_col,
        )
        .unwrap_or_else(|| target_bar.tabs.len().saturating_sub(1));

        match drag.reorder_mode {
            DragReorderMode::Live => {
                let from_bar = drag
                    .bar_id
                    .clone()
                    .expect("can_transfer guarantees source bar_id");
                let to_bar = target_bar
                    .bar_id
                    .clone()
                    .expect("can_transfer guarantees target bar_id");
                let event = DraggableTabTransferEvent {
                    from_bar,
                    to_bar,
                    from: drag.current_index,
                    to: to_index,
                };

                drag.id = target_id;
                drag.bar_id = target_bar.bar_id.clone();
                drag.current_index = to_index;
                drag.pending_id = target_id;
                drag.pending_bar_id = target_bar.bar_id.clone();
                drag.pending_index = to_index;
                drag.on_transfer = target_bar.on_transfer.clone().or(drag.on_transfer);

                Some((drag, Some(DraggableTabDragEvent::Transfer(event))))
            }
            DragReorderMode::OnDrop => {
                drag.pending_id = target_id;
                drag.pending_bar_id = target_bar.bar_id.clone();
                drag.pending_index = to_index;
                Some((drag, None))
            }
        }
    } else if target_id == drag.id {
        match drag.reorder_mode {
            DragReorderMode::Live => {
                let bar_disp_opts = bar.display_options();
                let bar_vp_opts = bar.viewport_options(target_inner.w as usize);
                let target = DraggableTabBar::adjacent_reorder_target_at_view_col(
                    &bar.tabs,
                    &bar_disp_opts,
                    &bar_vp_opts,
                    drag.current_index,
                    view_col,
                )
                .unwrap_or(drag.current_index);

                if target != drag.current_index {
                    let event = DraggableTabReorderEvent {
                        from: drag.current_index,
                        to: target,
                    };
                    drag.current_index = target;
                    drag.pending_id = drag.id;
                    drag.pending_bar_id = drag.bar_id.clone();
                    drag.pending_index = target;
                    Some((drag, Some(DraggableTabDragEvent::Reorder(event))))
                } else {
                    Some((drag, None))
                }
            }
            DragReorderMode::OnDrop => {
                let bar_disp_opts_ondrop = bar.display_options();
                let bar_vp_opts_ondrop = bar.viewport_options(target_inner.w as usize);
                let target =
                    crate::widgets::draggable_tab_bar::reorder_target_at_view_col_with_options(
                        &bar.tabs,
                        &bar_disp_opts_ondrop,
                        &bar_vp_opts_ondrop,
                        view_col,
                    )
                    .unwrap_or_else(|| {
                        if view_col == 0 {
                            0
                        } else {
                            bar.tabs
                                .iter()
                                .rposition(crate::widgets::draggable_tab_bar::is_reorderable_tab)
                                .unwrap_or_else(|| bar.tabs.len().saturating_sub(1))
                        }
                    });

                drag.pending_id = drag.id;
                drag.pending_bar_id = drag.bar_id.clone();
                drag.pending_index = target;
                Some((drag, None))
            }
        }
    } else {
        Some((drag, None))
    }
}

/// Compute the buffer-space rect of a single tab for snapshot preview.
fn compute_tab_snapshot_anchor(
    tree: &NodeTree,
    bar_id: NodeId,
    tab_index: usize,
) -> Option<crate::style::Rect> {
    if !tree.is_valid(bar_id) {
        return None;
    }
    let node = tree.node(bar_id);
    let NodeKind::DraggableTabBar(bar) = &node.kind else {
        return None;
    };
    let inner = node.rect.inner(bar.border, bar.padding);
    if inner.w == 0 || inner.h == 0 {
        return None;
    }
    let disp_opts = bar.display_options();
    let vp_opts = bar.viewport_options(inner.w as usize);
    let layout = DraggableTabBar::viewport_layout(&bar.tabs, &disp_opts, &vp_opts);
    let vis = layout.visible_tabs.iter().find(|v| v.index == tab_index)?;
    let w = vis.end.saturating_sub(vis.start);
    if w == 0 {
        return None;
    }
    Some(crate::style::Rect {
        x: inner
            .x
            .saturating_add(vis.start.min(i16::MAX as usize) as i16),
        y: inner.y,
        w: w.min(u16::MAX as usize) as u16,
        h: inner.h.max(1),
    })
}

/// Resolve final reorder event when drag ends.
pub(crate) fn finish_draggable_tab_bar_drag(
    drag: DraggableTabBarDrag,
) -> Option<DraggableTabDragEvent> {
    if !drag.started {
        return None;
    }

    match drag.reorder_mode {
        DragReorderMode::Live => None,
        DragReorderMode::OnDrop => {
            if drag.pending_id == drag.source_id {
                if drag.pending_index != drag.source_index {
                    Some(DraggableTabDragEvent::Reorder(DraggableTabReorderEvent {
                        from: drag.source_index,
                        to: drag.pending_index,
                    }))
                } else {
                    None
                }
            } else if let (Some(from_bar), Some(to_bar), Some(_)) = (
                drag.source_bar_id.clone(),
                drag.pending_bar_id.clone(),
                drag.on_transfer.clone(),
            ) {
                Some(DraggableTabDragEvent::Transfer(DraggableTabTransferEvent {
                    from_bar,
                    to_bar,
                    from: drag.source_index,
                    to: drag.pending_index,
                }))
            } else {
                None
            }
        }
    }
}

/// Handle textarea mouse drag selection.
#[allow(clippy::type_complexity)]
pub(crate) fn handle_textarea_drag(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
    anchor: usize,
) -> Option<(
    Arc<str>,
    usize,
    Option<usize>,
    Option<Callback<crate::widgets::TextAreaEvent>>,
    bool,
)> {
    if !tree.is_valid(id) {
        return None;
    }

    let node = tree.node(id);
    match &node.kind {
        NodeKind::TextArea(ta) => {
            let inner = node.rect.inner(ta.border, ta.padding);

            let parent_v_edge = tree.parent_frame_integrated_v_edge(id).unwrap_or(false);
            let parent_h_edge = tree.parent_frame_integrated_h_edge(id).unwrap_or(false);

            let scrollbar_over_border = ta.scrollbar
                && matches!(ta.scrollbar_variant, ScrollbarVariant::Integrated)
                && (ta.border || parent_v_edge);
            let h_scrollbar_over_border = ta.h_scrollbar
                && matches!(ta.h_scrollbar_variant, ScrollbarVariant::Integrated)
                && (ta.border || parent_h_edge);

            if inner.w == 0 || inner.h == 0 {
                return None;
            }

            let sentinel = crate::widgets::sentinel_info_for(
                ta.image_mode,
                ta.images.len(),
                &ta.image_placeholder,
                &ta.sentinels,
            );
            let visual_lines = {
                let (sentinel_ph_width, sentinel_count) = sentinel
                    .as_ref()
                    .and_then(|si| si.image.map(|(_, _, pw)| (pw, ta.images.len())))
                    .unwrap_or((0, 0));
                let custom_sentinel_hash: u64 = {
                    use std::hash::{Hash, Hasher};
                    let mut h = rustc_hash::FxHasher::default();
                    if let Some(si) = sentinel.as_ref()
                        && let Some((_, _, ref widths, _)) = si.custom
                    {
                        widths.hash(&mut h);
                    }
                    h.finish()
                };
                let key = crate::widgets::make_text_area_visual_key(
                    ta.content_hash,
                    crate::widgets::hash_peer_source_lines(ta.peer_source_lines.as_ref()),
                    crate::widgets::TextAreaVisualKeyArgs {
                        inner_w: inner.w,
                        wrap: ta.wrap,
                        line_numbers: ta.line_numbers,
                        min_line_number_width: ta.min_line_number_width,
                        scrollbar: ta.scrollbar,
                        scrollbar_over_border,
                        scrollbar_gap: ta.scrollbar_gap,
                        read_only: ta.read_only,
                        cursor: ta.cursor,
                        tab_stop: ta.tab_display_width,
                        sentinel_ph_width,
                        sentinel_count,
                        custom_sentinel_hash,
                        virtual_text_hash: crate::widgets::text_area_virtual_text_hash(
                            &ta.virtual_texts,
                        ),
                        gutter_col_width: ta.gutter_col_width,
                        gutter_gap: ta.gutter_gap,
                        #[cfg(feature = "diff-view")]
                        split_wrap_pane_widths: if let (Some(sync), Some(side)) =
                            (&ta.split_wrap_sync, ta.split_wrap_side)
                        {
                            crate::widgets::split_wrap_pane_widths(sync, side)
                        } else {
                            None
                        },
                        #[cfg(feature = "diff-view")]
                        split_wrap_scrollbar_cols: ta
                            .split_wrap_sync
                            .as_ref()
                            .map(crate::widgets::split_wrap_scrollbar_cols_pair),
                        #[cfg(feature = "diff-view")]
                        split_wrap_layout_pass: ta
                            .split_wrap_sync
                            .as_ref()
                            .map(crate::widgets::split_wrap_layout_pass)
                            .unwrap_or(0),
                    },
                );
                ta.visual_cache.get_lines(&key)
            };
            let new_cursor = crate::app::input::text::textarea_cursor_from_coords(
                crate::app::input::text::TextAreaCursorParams {
                    value: ta.value.as_ref(),
                    current_cursor: ta.cursor,
                    coords: crate::app::input::text::TextAreaCursorCoords {
                        x,
                        y,
                        inner,
                        clamp_to_inner: false,
                    },
                    layout: crate::app::input::text::TextAreaCursorLayout {
                        line_numbers: ta.line_numbers,
                        min_line_number_width: ta.min_line_number_width,
                        wrap: ta.wrap,
                        scroll_offset: ta.scroll_offset,
                        scrollbar: ta.scrollbar,
                        scrollbar_variant: ta.scrollbar_variant,
                        scrollbar_gap: ta.scrollbar_gap,
                        scrollbar_over_border,
                        h_scrollbar: ta.h_scrollbar,
                        h_scrollbar_variant: ta.h_scrollbar_variant,
                        h_scrollbar_over_border,
                        max_line_width: ta.max_line_width,
                        h_scroll_offset: ta.h_scroll_offset,
                        tab_stop: ta.tab_display_width as usize,
                        gutter_col_width: ta.gutter_col_width,
                        gutter_gap: ta.gutter_gap,
                        logical_lines_count: ta.logical_lines_count,
                    },
                    read_only: ta.read_only,
                    sentinel,
                    visual_lines,
                    virtual_texts: &ta.virtual_texts,
                },
            );

            Some((
                ta.value.clone(),
                new_cursor,
                Some(anchor),
                ta.on_change.clone(),
                ta.read_only,
            ))
        }
        _ => None,
    }
}

/// Handle input mouse drag selection.
#[allow(clippy::type_complexity)]
pub(crate) fn handle_input_drag(
    tree: &NodeTree,
    x: u16,
    id: NodeId,
    anchor: usize,
) -> Option<(
    Arc<str>,
    usize,
    Option<usize>,
    Option<Callback<InputEvent>>,
    bool,
)> {
    if !tree.is_valid(id) {
        return None;
    }

    let node = tree.node(id);
    match &node.kind {
        NodeKind::Input(input_node) => {
            let inner = node.rect.inner(input_node.border, input_node.padding);

            if inner.w == 0 {
                return None;
            }

            let new_cursor = crate::app::input::text::input_cursor_from_coords(
                &input_node.value,
                input_node.prefix.as_deref(),
                x,
                input_node.cursor,
                inner,
            );

            Some((
                input_node.value.clone(),
                new_cursor,
                Some(anchor),
                input_node.on_change.clone(),
                input_node.read_only,
            ))
        }
        _ => None,
    }
}

/// Handle hex area mouse drag selection.
#[allow(clippy::type_complexity)]
pub(crate) fn handle_hex_area_drag(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
    anchor: usize,
) -> Option<(
    usize,
    Option<usize>,
    Option<Callback<crate::widgets::HexAreaCursorEvent>>,
)> {
    if !tree.is_valid(id) {
        return None;
    }

    let node = tree.node(id);
    match &node.kind {
        NodeKind::HexArea(hex) => {
            if hex.disabled || hex.bytes.is_empty() {
                return None;
            }

            let hit = crate::widgets::pointer_hit(
                node.rect,
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
            )?;

            Some((
                hit.index,
                Some(anchor.min(hex.bytes.len().saturating_sub(1))),
                hex.on_cursor_change.clone(),
            ))
        }
        _ => None,
    }
}

/// Resolve a flattened document byte cursor from viewport coordinates.
pub(crate) fn document_view_cursor_from_coords(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
) -> Option<usize> {
    if !tree.is_valid(id) {
        return None;
    }

    let node = tree.node(id);
    let NodeKind::DocumentView(doc) = &node.kind else {
        return None;
    };

    let rect = node.rect;
    let inner = rect.inner(doc.border, doc.padding);
    let cl = doc.content_layout(inner);

    if cl.content_width == 0 || cl.content_height == 0 {
        return Some(doc.selection_cursor.min(doc.visual_cache.flat_text.len()));
    }

    let content_x = cl.content_x;
    let content_y = cl.content_y;
    let content_w = cl.content_width;
    let content_h = cl.content_height;
    let max_x = content_x.saturating_add(content_w.saturating_sub(1) as i16);
    let max_y = content_y.saturating_add(content_h.saturating_sub(1) as i16);

    // Extend selection beyond viewport when dragging out of bounds.
    let above = (y as i16) < content_y;
    let below = (y as i16) > max_y;
    let left_of_content = (x as i16) < content_x;

    if above {
        return Some(0);
    }

    if below {
        return Some(doc.visual_cache.flat_text.len());
    }

    let clamped_x = (x as i16).clamp(content_x, max_x);
    let clamped_y = (y as i16).clamp(content_y, max_y);

    let visual_idx = if above {
        0
    } else {
        let rel_y = clamped_y.saturating_sub(content_y) as usize;
        doc.scroll_offset.saturating_add(rel_y)
    };
    if visual_idx >= doc.visual_cache.line_texts.len() {
        return Some(doc.visual_cache.flat_text.len());
    }

    let rel_x = clamped_x.saturating_sub(content_x).max(0) as usize;
    let visual_col = if left_of_content {
        0
    } else if !doc.wrap {
        rel_x.saturating_add(doc.h_scroll_offset)
    } else {
        rel_x
    };
    let line_text = doc.visual_cache.line_texts[visual_idx].as_ref();
    let vline = doc.visual_cache.lines.get(visual_idx)?;
    if matches!(&vline.kind, VisualLineKind::DiagramRow { .. }) {
        let line_start = doc
            .visual_cache
            .line_starts
            .get(visual_idx)
            .copied()
            .unwrap_or(0);
        let line_len = doc
            .visual_cache
            .line_lengths
            .get(visual_idx)
            .copied()
            .unwrap_or(0);
        return Some(if visual_col == 0 {
            line_start
        } else {
            line_start.saturating_add(line_len)
        });
    }
    let byte_in_line =
        crate::widgets::document_view::node::document_view_byte_in_line_from_visual_col(
            line_text, vline, visual_col,
        );

    let line_start = doc
        .visual_cache
        .line_starts
        .get(visual_idx)
        .copied()
        .unwrap_or(0);
    Some(line_start.saturating_add(byte_in_line))
}
