use crate::core::element::Key;
use crate::core::node::{Node, NodeId, NodeKind, NodeTree, ScrollbarAxis};
use crate::style::{Rect, ScrollbarVariant};
use crate::utils::math::round_mul_div;
use crate::utils::scrollbar::ScrollbarMetrics as CoreScrollbarMetrics;
use crate::widgets::ScrollEvent;
use crate::widgets::internal::scroll_metrics;
use crate::widgets::list::utils::{list_scrollbar_metrics, list_scrollbar_metrics_half};
use crate::widgets::{
    calc_scroll_view_window, scroll_view_scrollbar_metrics, text_area_cursor_reserve,
    text_area_total_gutter_width,
};

const DEFAULT_SCROLLBAR_THUMB: char = '█';

#[derive(Clone, Debug)]
pub(crate) struct ScrollbarDrag {
    pub id: NodeId,
    pub key: Option<Key>,
    pub axis: crate::core::node::ScrollbarAxis,
    pub grab_offset: u16,
    pub grab_subcell: u8,
}

pub(crate) struct ScrollbarMetrics {
    pub inner: Rect,
    pub core: CoreScrollbarMetrics,
    pub half_cell: bool,
}

fn use_half_thumb(thumb: Option<char>) -> bool {
    thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB) == DEFAULT_SCROLLBAR_THUMB
}

fn drag_cursor_units(
    cursor_cell: usize,
    track_cells: usize,
    half_cell: bool,
    grab_subcell: u8,
) -> usize {
    if half_cell {
        if cursor_cell == 0 {
            return 0;
        }
        if cursor_cell.saturating_add(1) >= track_cells {
            return track_cells.saturating_mul(2).saturating_sub(1);
        }
        cursor_cell
            .saturating_mul(2)
            .saturating_add(grab_subcell.min(1) as usize)
    } else {
        cursor_cell
    }
}

fn drag_start_position(
    cursor_cell: usize,
    thumb_start: usize,
    thumb_len: usize,
    half_cell: bool,
) -> (u8, usize) {
    if !half_cell {
        let thumb_end = thumb_start.saturating_add(thumb_len);
        let grab_offset = if cursor_cell <= thumb_start {
            0
        } else if cursor_cell >= thumb_end {
            thumb_len.saturating_sub(1)
        } else {
            cursor_cell.saturating_sub(thumb_start)
        };
        return (0, grab_offset);
    }

    let thumb_end = thumb_start.saturating_add(thumb_len);
    let cell_start = cursor_cell.saturating_mul(2);
    let cell_end = cell_start.saturating_add(2);

    if cell_end <= thumb_start {
        return (0, 0);
    }
    if cell_start >= thumb_end {
        return (1, thumb_len.saturating_sub(1));
    }

    let overlap_start = thumb_start.max(cell_start);
    let overlap_end = thumb_end.min(cell_end);
    let cell_center = cell_start.saturating_add(1);
    let cursor_units = cell_center.clamp(overlap_start, overlap_end.saturating_sub(1));
    let grab_subcell = cursor_units.saturating_sub(cell_start).min(1) as u8;
    let grab_offset = cursor_units.saturating_sub(thumb_start);
    (grab_subcell, grab_offset)
}

fn cursor_on_track(axis: ScrollbarAxis, x: u16, y: u16, inner: Rect) -> Option<usize> {
    let (pos, len) = match axis {
        ScrollbarAxis::Vertical => (y as i32 - inner.y as i32, inner.h as i32),
        ScrollbarAxis::Horizontal => (x as i32 - inner.x as i32, inner.w as i32),
    };

    if len <= 0 {
        return None;
    }

    let cursor = pos.clamp(0, len.saturating_sub(1)) as usize;
    Some(cursor)
}

pub(crate) fn compute_metrics(node: &Node, axis: ScrollbarAxis) -> Option<ScrollbarMetrics> {
    match axis {
        ScrollbarAxis::Vertical => match &node.kind {
            NodeKind::List(list_node) => {
                if !list_node.scrollbar || list_node.disabled {
                    return None;
                }

                let inner = node.rect.inner(list_node.border, list_node.padding);

                let total = list_node.items.len();
                if inner.h == 0 || total == 0 {
                    return None;
                }

                let track_h = inner.h as usize;
                let half_cell = use_half_thumb(list_node.scrollbar_thumb);
                let core = if half_cell {
                    list_scrollbar_metrics_half(
                        &list_node.items,
                        list_node.offset,
                        track_h,
                        list_node.show_scroll_indicators,
                    )?
                } else {
                    list_scrollbar_metrics(
                        &list_node.items,
                        list_node.offset,
                        track_h,
                        list_node.show_scroll_indicators,
                    )?
                };

                Some(ScrollbarMetrics {
                    inner,
                    core,
                    half_cell,
                })
            }
            NodeKind::ScrollView(scroll_view) => {
                if !scroll_view.scrollbar {
                    return None;
                }

                let mut inner = node
                    .rect
                    .inner(scroll_view.props.border, scroll_view.props.padding);

                // Mirror the renderer: a standalone horizontal scrollbar reserves
                // the bottom row, so the vertical track is one row shorter. Keep
                // this in sync or scrollbar drags land a row off.
                let h_integrated = scroll_view.props.border
                    && scroll_view.h_scrollbar
                    && matches!(
                        scroll_view.h_scrollbar_variant,
                        ScrollbarVariant::Integrated
                    );
                let reserve_bottom =
                    if scroll_view.h_scrollbar && scroll_view.h_max_offset > 0 && !h_integrated {
                        1u16.saturating_add(scroll_view.h_scrollbar_gap)
                    } else {
                        0
                    };
                inner.h = inner.h.saturating_sub(reserve_bottom);

                let total = scroll_view.content_height as usize;
                // Keep the viewport (thumb size + scroll range) at the full height so
                // it matches reconcile's `max_offset`; only the physical track above
                // shrank. Reducing this too would leave the fully-scrolled thumb short.
                let viewport_h = scroll_view.viewport_height as usize;
                if inner.h == 0 || total <= viewport_h {
                    return None;
                }

                let half_cell = use_half_thumb(scroll_view.scrollbar_thumb);
                let core = scroll_view_scrollbar_metrics(
                    scroll_view.scroll_offset as usize,
                    total,
                    viewport_h,
                    inner.h as usize,
                    scroll_view.show_scroll_indicators,
                    half_cell,
                )?;

                Some(ScrollbarMetrics {
                    inner: Rect {
                        x: inner.x,
                        y: inner.y,
                        w: 1,
                        h: inner.h,
                    },
                    core,
                    half_cell,
                })
            }
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(term) => {
                if !term.scrollbar || term.total_scrollback_rows == 0 {
                    return None;
                }

                let inner = node.rect.inner(term.border, term.padding);
                let total = term.viewport_rows + term.total_scrollback_rows;
                let visible = term.viewport_rows;

                if inner.h == 0 || visible == 0 || total <= visible {
                    return None;
                }

                let std_offset = term
                    .total_scrollback_rows
                    .saturating_sub(term.scrollback_offset);
                let half_cell = use_half_thumb(term.scrollbar_thumb);
                let core = if half_cell {
                    CoreScrollbarMetrics::new_with_half_track(
                        total,
                        visible,
                        std_offset,
                        inner.h as usize,
                    )
                } else {
                    CoreScrollbarMetrics::new_with_track(
                        total,
                        visible,
                        std_offset,
                        inner.h as usize,
                    )
                };

                Some(ScrollbarMetrics {
                    inner: Rect {
                        x: inner.x,
                        y: inner.y,
                        w: 1,
                        h: inner.h,
                    },
                    core,
                    half_cell,
                })
            }
            NodeKind::TextArea(ta) => {
                if !ta.scrollbar || ta.disabled {
                    return None;
                }
                let mut inner = node.rect.inner(ta.border, ta.padding);

                let logical_lines_count = if ta.value.is_empty() {
                    1
                } else {
                    ta.value.split('\n').count()
                };

                let gutter_width = text_area_total_gutter_width(
                    logical_lines_count,
                    ta.line_numbers,
                    ta.min_line_number_width,
                    ta.gutter_col_width,
                    ta.gutter_gap,
                ) as usize;

                let scrollbar_over_border = ta.scrollbar
                    && matches!(ta.scrollbar_variant, ScrollbarVariant::Integrated)
                    && ta.border;
                let scrollbar_cols = if ta.scrollbar && !scrollbar_over_border {
                    1u16.saturating_add(ta.scrollbar_gap)
                } else {
                    0
                };

                let content_width = inner
                    .w
                    .saturating_sub(gutter_width as u16)
                    .saturating_sub(scrollbar_cols)
                    .saturating_sub(text_area_cursor_reserve(ta.wrap, ta.read_only))
                    as usize;

                let h_scrollbar_over_border = ta.h_scrollbar
                    && matches!(ta.h_scrollbar_variant, ScrollbarVariant::Integrated)
                    && ta.border;
                let h_scrollbar_visible =
                    ta.h_scrollbar && !ta.wrap && ta.max_line_width > content_width;

                if h_scrollbar_visible && !h_scrollbar_over_border {
                    inner.h = inner.h.saturating_sub(1);
                }

                let total = ta.visual_lines_count;
                let visible = inner.h as usize;
                if inner.h == 0 || total <= visible {
                    return None;
                }

                let half_cell = use_half_thumb(ta.scrollbar_thumb);
                let core = if half_cell {
                    CoreScrollbarMetrics::new_with_half_track(
                        total,
                        visible,
                        ta.scroll_offset,
                        inner.h as usize,
                    )
                } else {
                    CoreScrollbarMetrics::new_with_track(
                        total,
                        visible,
                        ta.scroll_offset,
                        inner.h as usize,
                    )
                };

                Some(ScrollbarMetrics {
                    inner,
                    core,
                    half_cell,
                })
            }
            NodeKind::DocumentView(dv) => {
                if !dv.scrollbar {
                    return None;
                }

                let inner = node.rect.inner(dv.border, dv.padding);
                if inner.h == 0 {
                    return None;
                }

                let cl = dv.content_layout(inner);

                let inner_h = cl.content_height;
                let total = dv.total_visual_lines;
                let visible = inner_h as usize;
                if inner_h == 0 || total <= visible {
                    return None;
                }

                let half_cell = use_half_thumb(dv.scrollbar_thumb);
                let core = if half_cell {
                    CoreScrollbarMetrics::new_with_half_track(
                        total,
                        visible,
                        dv.scroll_offset,
                        inner_h as usize,
                    )
                } else {
                    CoreScrollbarMetrics::new_with_track(
                        total,
                        visible,
                        dv.scroll_offset,
                        inner_h as usize,
                    )
                };

                Some(ScrollbarMetrics {
                    inner: Rect {
                        x: inner.x,
                        y: inner.y,
                        w: 1,
                        h: inner_h,
                    },
                    core,
                    half_cell,
                })
            }
            _ => None,
        },
        ScrollbarAxis::Horizontal => match &node.kind {
            NodeKind::TextArea(ta) => {
                if !ta.h_scrollbar || ta.disabled || ta.wrap {
                    return None;
                }

                let mut inner = node.rect.inner(ta.border, ta.padding);

                let logical_lines_count = if ta.value.is_empty() {
                    1
                } else {
                    ta.value.split('\n').count()
                };

                let gutter_width = text_area_total_gutter_width(
                    logical_lines_count,
                    ta.line_numbers,
                    ta.min_line_number_width,
                    ta.gutter_col_width,
                    ta.gutter_gap,
                ) as usize;

                let v_scrollbar_over_border = ta.scrollbar
                    && matches!(ta.scrollbar_variant, ScrollbarVariant::Integrated)
                    && ta.border;
                let scrollbar_cols = if ta.scrollbar && !v_scrollbar_over_border {
                    1u16.saturating_add(ta.scrollbar_gap)
                } else {
                    0
                };

                let content_width = inner
                    .w
                    .saturating_sub(gutter_width as u16)
                    .saturating_sub(scrollbar_cols)
                    .saturating_sub(text_area_cursor_reserve(ta.wrap, ta.read_only))
                    as usize;

                let h_scrollbar_over_border =
                    ta.h_scrollbar_variant == ScrollbarVariant::Integrated && ta.border;
                if !h_scrollbar_over_border {
                    inner.h = inner.h.saturating_sub(1);
                }

                if inner.w == 0 || inner.h == 0 {
                    return None;
                }

                if ta.max_line_width <= content_width || content_width == 0 {
                    return None;
                }

                let track_rect = Rect {
                    x: inner.x.saturating_add(gutter_width as i16),
                    y: inner.y.saturating_add(inner.h as i16),
                    w: content_width as u16,
                    h: 1,
                };

                let half_cell = use_half_thumb(ta.h_scrollbar_thumb);
                let core = if half_cell {
                    CoreScrollbarMetrics::new_with_half_track(
                        ta.max_line_width,
                        content_width,
                        ta.h_scroll_offset,
                        content_width,
                    )
                } else {
                    CoreScrollbarMetrics::new_with_track(
                        ta.max_line_width,
                        content_width,
                        ta.h_scroll_offset,
                        content_width,
                    )
                };

                Some(ScrollbarMetrics {
                    inner: track_rect,
                    core,
                    half_cell,
                })
            }
            NodeKind::DocumentView(dv) => {
                if !dv.h_scrollbar || dv.wrap {
                    return None;
                }

                let inner = node.rect.inner(dv.border, dv.padding);
                let cl = dv.content_layout(inner);
                let content_width = cl.content_width as usize;

                if inner.w == 0 || inner.h == 0 {
                    return None;
                }

                if (dv.max_line_width as usize) <= content_width || content_width == 0 {
                    return None;
                }

                let h_y = if cl.h_scrollbar_over_border {
                    // sits on the bottom border row
                    inner.y.saturating_add(inner.h as i16)
                } else {
                    // standalone: sits below content, within inner
                    inner.y.saturating_add(inner.h.saturating_sub(1) as i16)
                };

                let track_rect = Rect {
                    x: cl.content_x,
                    y: h_y,
                    w: content_width as u16,
                    h: 1,
                };

                let half_cell = use_half_thumb(dv.h_scrollbar_thumb);
                let core = if half_cell {
                    CoreScrollbarMetrics::new_with_half_track(
                        dv.max_line_width as usize,
                        content_width,
                        dv.h_scroll_offset,
                        content_width,
                    )
                } else {
                    CoreScrollbarMetrics::new_with_track(
                        dv.max_line_width as usize,
                        content_width,
                        dv.h_scroll_offset,
                        content_width,
                    )
                };

                Some(ScrollbarMetrics {
                    inner: track_rect,
                    core,
                    half_cell,
                })
            }
            NodeKind::ScrollView(scroll_view) => {
                if !scroll_view.h_scrollbar
                    || !scroll_view.axis.horizontal_enabled()
                    || scroll_view.h_max_offset == 0
                {
                    return None;
                }

                let inner = node
                    .rect
                    .inner(scroll_view.props.border, scroll_view.props.padding);
                if inner.w == 0 || inner.h == 0 {
                    return None;
                }

                let use_standalone_v = scroll_view.scrollbar
                    && matches!(scroll_view.scrollbar_variant, ScrollbarVariant::Standalone);
                let mut content_inner = inner;
                if use_standalone_v && content_inner.w > 0 {
                    content_inner.w = content_inner
                        .w
                        .saturating_sub(1u16.saturating_add(scroll_view.scrollbar_gap));
                }
                if scroll_view.show_scroll_indicators {
                    if scroll_view.top_indicator {
                        content_inner.y = content_inner.y.saturating_add(1);
                        content_inner.h = content_inner.h.saturating_sub(1);
                    }
                    if scroll_view.bottom_indicator {
                        content_inner.h = content_inner.h.saturating_sub(1);
                    }
                }

                let total = scroll_view.content_width as usize;
                let viewport_w = scroll_view.viewport_width as usize;
                if total <= viewport_w || viewport_w == 0 {
                    return None;
                }

                let h_integrated = scroll_view.props.border
                    && matches!(
                        scroll_view.h_scrollbar_variant,
                        ScrollbarVariant::Integrated
                    );
                let track_rect = if h_integrated {
                    Rect {
                        x: content_inner.x,
                        y: inner.y.saturating_add(inner.h.saturating_sub(1) as i16),
                        w: content_inner.w,
                        h: 1,
                    }
                } else {
                    Rect {
                        x: content_inner.x,
                        y: content_inner.y.saturating_add(content_inner.h as i16),
                        w: content_inner.w,
                        h: 1,
                    }
                };

                let track_w = content_inner.w as usize;
                let half_cell = use_half_thumb(scroll_view.h_scrollbar_thumb);
                let core = if half_cell {
                    CoreScrollbarMetrics::new_with_half_track(
                        total,
                        viewport_w,
                        scroll_view.h_offset,
                        track_w,
                    )
                } else {
                    CoreScrollbarMetrics::new_with_track(
                        total,
                        viewport_w,
                        scroll_view.h_offset,
                        track_w,
                    )
                };

                Some(ScrollbarMetrics {
                    inner: track_rect,
                    core,
                    half_cell,
                })
            }
            _ => None,
        },
    }
}

pub(crate) fn start_drag(
    node: &Node,
    axis: ScrollbarAxis,
    x: u16,
    y: u16,
) -> Option<ScrollbarDrag> {
    let metrics = compute_metrics(node, axis)?;
    let cursor = cursor_on_track(axis, x, y, metrics.inner)?;
    let (grab_subcell, grab_offset) = drag_start_position(
        cursor,
        metrics.core.thumb_start,
        metrics.core.thumb_len,
        metrics.half_cell,
    );

    Some(ScrollbarDrag {
        id: node.id,
        key: node.key.clone(),
        axis,
        grab_offset: grab_offset as u16,
        grab_subcell,
    })
}

pub(crate) fn rebind_drag_to_key(tree: &NodeTree, drag: &mut ScrollbarDrag) -> bool {
    let Some(key) = drag.key.as_ref() else {
        return false;
    };
    let Some(node) = tree
        .iter_with_overlays()
        .find(|node| node.key.as_ref() == Some(key) && compute_metrics(node, drag.axis).is_some())
    else {
        return false;
    };
    drag.id = node.id;
    true
}

pub(crate) fn remember_scroll_view_input_offset(tree: &mut NodeTree, id: NodeId) {
    if !tree.is_valid(id) {
        return;
    }
    let node = tree.node(id);
    let Some(key) = node.key.clone() else {
        return;
    };
    let NodeKind::ScrollView(scroll) = &node.kind else {
        return;
    };
    tree.scroll_input_offset_by_key.insert(key, scroll.offset);
}

pub(crate) fn handle_drag(
    node: &mut Node,
    axis: ScrollbarAxis,
    x: u16,
    y: u16,
    grab_offset: u16,
    grab_subcell: u8,
) -> bool {
    let metrics = match compute_metrics(node, axis) {
        Some(m) => m,
        None => return false,
    };
    let cursor = match cursor_on_track(axis, x, y, metrics.inner) {
        Some(cursor) => cursor,
        None => return false,
    };

    let track_cells = match axis {
        ScrollbarAxis::Vertical => metrics.inner.h as usize,
        ScrollbarAxis::Horizontal => metrics.inner.w as usize,
    };
    let cursor_units = drag_cursor_units(cursor, track_cells, metrics.half_cell, grab_subcell);
    let mut thumb_start = cursor_units.saturating_sub(grab_offset as usize);
    thumb_start = thumb_start.min(metrics.core.max_thumb_start);

    let max_offset = metrics.core.max_offset;
    let new_offset = if metrics.core.max_thumb_start == 0 {
        0
    } else {
        round_mul_div(max_offset, thumb_start, metrics.core.max_thumb_start)
    };

    match axis {
        ScrollbarAxis::Vertical => match &mut node.kind {
            NodeKind::List(list_node) => {
                // When scroll indicators are active, the forward mapping uses compressed
                // offsets (offset-1 mapped to max_offset-1). The inverse must decompress
                // to stay symmetric and avoid landing on the forbidden offset=1.
                let list_offset = if list_node.show_scroll_indicators
                    && max_offset > 1
                    && thumb_start > 0
                {
                    let compressed_max = max_offset.saturating_sub(1);
                    let compressed =
                        round_mul_div(compressed_max, thumb_start, metrics.core.max_thumb_start);
                    // Decompress: shift by 1 (inverse of forward's -1), minimum 2
                    // to skip the forbidden offset=1 slot.
                    (compressed + 1).max(2).min(max_offset)
                } else if thumb_start == 0 {
                    0
                } else {
                    new_offset
                };

                if list_offset != list_node.offset {
                    list_node.offset = list_offset;
                    list_node.scroll_override = Some(list_offset);
                }

                // Keep indicator state consistent immediately (drag mutates offset without reconcile).
                let inner = node.rect.inner(list_node.border, list_node.padding);
                let visible = inner.h as usize;
                let total = list_node.items.len();
                if list_node.show_scroll_indicators && visible > 0 && total > 0 {
                    let (_s, e, t, b) =
                        crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                            list_node.offset,
                            &list_node.items,
                            visible,
                            true,
                        );
                    list_node.top_indicator = t;
                    list_node.bottom_indicator = b;
                    list_node.bottom_count = total.saturating_sub(e);
                } else {
                    list_node.top_indicator = false;
                    list_node.bottom_indicator = false;
                    list_node.bottom_count = 0;
                }
                true
            }
            NodeKind::Table(table_node) => {
                if new_offset != table_node.offset {
                    table_node.offset = new_offset;
                    table_node.scroll_override = Some(new_offset);
                }
                true
            }
            NodeKind::ScrollView(scroll_view) => {
                let new_row = if scroll_view.show_scroll_indicators
                    && metrics.core.max_offset > 1
                    && thumb_start > 0
                {
                    let compressed_max = metrics.core.max_offset.saturating_sub(1);
                    let compressed =
                        round_mul_div(compressed_max, thumb_start, metrics.core.max_thumb_start);
                    (compressed + 1).max(2).min(metrics.core.max_offset)
                } else if thumb_start == 0 {
                    0
                } else {
                    new_offset.min(scroll_view.max_offset)
                };
                scroll_view.smooth_scroll.cancel_at(new_row);
                scroll_view.cancelled_scroll_target = scroll_view.scroll_target.clone();
                if new_row != scroll_view.offset {
                    scroll_view.offset = new_row;
                    scroll_view.scroll_override = Some(new_row);
                    scroll_view.scroll_handler_dirty = true;
                }

                if let Some(cb) = scroll_view.on_scroll_to.clone() {
                    cb.emit(new_row);
                    return true;
                }

                let Some(cb) = scroll_view.on_scroll.clone() else {
                    return true;
                };

                let total = scroll_view.content_height as usize;
                let viewport_h = scroll_view.viewport_height as usize;
                let visible_for_scroll = calc_scroll_view_window(
                    new_row,
                    total,
                    viewport_h,
                    scroll_view.show_scroll_indicators,
                )
                .visible_rows;

                let metrics = scroll_metrics(total, visible_for_scroll, scroll_view.offset);
                cb.emit(ScrollEvent {
                    offset: new_row,
                    metrics,
                });
                true
            }
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(term) => {
                let new_offset = new_offset.min(term.total_scrollback_rows);
                let next_scrollback = term.total_scrollback_rows.saturating_sub(new_offset);

                if next_scrollback != term.scrollback_offset {
                    term.scrollback_offset = next_scrollback;
                    term.scroll_override = Some(next_scrollback);
                }

                if let Some(cb) = term.on_scroll_to.clone() {
                    cb.emit(next_scrollback);
                    return true;
                }

                let Some(cb) = term.on_scroll.clone() else {
                    return true;
                };

                let total = term.viewport_rows + term.total_scrollback_rows;
                let visible = term.viewport_rows;
                let metrics = scroll_metrics(total, visible, next_scrollback);
                cb.emit(ScrollEvent {
                    offset: next_scrollback,
                    metrics,
                });
                true
            }
            NodeKind::TextArea(ta) => {
                let new_offset = new_offset.min(metrics.core.max_offset);
                let line_target_cancel_pending =
                    ta.scroll_to_line.is_some() && ta.cancelled_scroll_to_line != ta.scroll_to_line;
                if new_offset != ta.scroll_offset || line_target_cancel_pending {
                    ta.smooth_scroll.cancel_at(new_offset);
                    ta.cancelled_scroll_to_line = ta.scroll_to_line;
                    if new_offset != ta.scroll_offset {
                        ta.scroll_offset = new_offset;
                        ta.scroll_override = Some(new_offset);
                    }
                }

                if let Some(cb) = ta.on_scroll_to.clone() {
                    cb.emit(new_offset);
                    return true;
                }

                let Some(cb) = ta.on_scroll.clone() else {
                    return true;
                };

                let visible = metrics.inner.h as usize;
                let metrics_data = scroll_metrics(ta.visual_lines_count, visible, ta.scroll_offset);
                cb.emit(ScrollEvent {
                    offset: new_offset,
                    metrics: metrics_data,
                });
                true
            }
            NodeKind::DocumentView(dv) => {
                let new_offset = new_offset.min(metrics.core.max_offset);
                let source_target_cancel_pending = dv.scroll_to_source_line.is_some()
                    && dv.cancelled_scroll_to_source_line != dv.scroll_to_source_line;
                if new_offset != dv.scroll_offset || source_target_cancel_pending {
                    dv.smooth_scroll.cancel_at(new_offset);
                    dv.cancelled_scroll_to_source_line = dv.scroll_to_source_line;
                    dv.scroll_offset = new_offset;
                    dv.scroll_override = Some(new_offset);
                }

                if let Some(cb) = dv.on_scroll.clone() {
                    let visible = metrics.inner.h as usize;
                    let metrics_data =
                        scroll_metrics(dv.total_visual_lines, visible, dv.scroll_offset);
                    cb.emit(ScrollEvent {
                        offset: new_offset,
                        metrics: metrics_data,
                    });
                }
                true
            }
            _ => false,
        },
        ScrollbarAxis::Horizontal => match &mut node.kind {
            NodeKind::TextArea(ta) => {
                let new_offset = new_offset.min(max_offset);
                let line_target_cancel_pending =
                    ta.scroll_to_line.is_some() && ta.cancelled_scroll_to_line != ta.scroll_to_line;
                if new_offset != ta.h_scroll_offset || line_target_cancel_pending {
                    ta.smooth_scroll.cancel_at(ta.scroll_offset);
                    ta.cancelled_scroll_to_line = ta.scroll_to_line;
                }
                if new_offset != ta.h_scroll_offset {
                    ta.h_scroll_offset = new_offset;
                    ta.h_scroll_override = Some(new_offset);
                }
                true
            }
            NodeKind::DocumentView(dv) => {
                let new_offset = new_offset.min(max_offset);
                let source_target_cancel_pending = dv.scroll_to_source_line.is_some()
                    && dv.cancelled_scroll_to_source_line != dv.scroll_to_source_line;
                if new_offset != dv.h_scroll_offset || source_target_cancel_pending {
                    dv.smooth_scroll.cancel_at(dv.scroll_offset);
                    dv.cancelled_scroll_to_source_line = dv.scroll_to_source_line;
                }
                if new_offset != dv.h_scroll_offset {
                    dv.h_scroll_offset = new_offset;
                    dv.h_scroll_override = Some(new_offset);
                }
                true
            }
            NodeKind::ScrollView(scroll_view) => {
                let new_offset = new_offset.min(scroll_view.h_max_offset);
                if new_offset != scroll_view.h_offset {
                    scroll_view.h_offset = new_offset;
                    scroll_view.h_scroll_offset = new_offset as u16;
                    scroll_view.h_scroll_override = Some(new_offset);
                    scroll_view.h_scroll_handler_dirty = true;
                }
                true
            }
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::Length;
    use crate::animation::{Easing, TransitionConfig};
    use crate::core::node::{NodeKind, NodeTree, ScrollbarAxis};
    use crate::layout::LayoutEngine;
    use crate::style::Rect;
    use crate::widgets::{DocumentView, ScrollBehavior, ScrollView, Text, TextArea};

    use super::{compute_metrics, drag_cursor_units, handle_drag};

    fn linear_smooth() -> ScrollBehavior {
        ScrollBehavior::smooth(TransitionConfig {
            duration: Duration::from_millis(100),
            easing: Easing::Linear,
        })
    }

    fn numbered_lines(count: usize) -> String {
        (0..count)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn reconcile(root: crate::Element, width: u16, height: u16) -> NodeTree {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            },
            None,
        );
        tree
    }

    #[test]
    fn half_cell_drag_reaches_full_track_extremes() {
        assert_eq!(drag_cursor_units(0, 10, true, 1), 0);
        assert_eq!(drag_cursor_units(9, 10, true, 0), 19);
        assert_eq!(drag_cursor_units(9, 10, true, 1), 19);
        assert_eq!(drag_cursor_units(4, 10, true, 0), 8);
        assert_eq!(drag_cursor_units(4, 10, true, 1), 9);
    }

    #[test]
    fn half_cell_drag_last_track_cell_can_reach_max_thumb_start() {
        let cursor_units = drag_cursor_units(9, 10, true, 0);
        let thumb_len = 4;
        let max_thumb_start = 20 - thumb_len;
        let grab_offset = thumb_len - 1;

        let thumb_start = cursor_units
            .saturating_sub(grab_offset)
            .min(max_thumb_start);
        assert_eq!(thumb_start, max_thumb_start);
    }

    #[test]
    fn vertical_scrollbar_track_reserves_horizontal_scrollbar_row() {
        let root: crate::Element = ScrollView::new()
            .axis(crate::widgets::ScrollAxis::Both)
            .scrollbar(true)
            .h_scrollbar(true)
            .children((0..30).map(|_| {
                Text::new("x".repeat(60))
                    .width(Length::Auto)
                    .height(Length::Px(1))
                    .into()
            }))
            .into();
        let tree = reconcile(root, 20, 10);
        let node = tree.node(tree.root);
        let NodeKind::ScrollView(sv) = &node.kind else {
            panic!("expected scroll view");
        };
        assert!(sv.h_max_offset > 0, "expected horizontal overflow");

        let metrics =
            compute_metrics(node, ScrollbarAxis::Vertical).expect("vertical scrollbar metrics");
        // Inner height is 10; the standalone horizontal scrollbar reserves the
        // bottom row, so the vertical track spans 9 rows (leaving a clean corner)
        // and the drag geometry matches the render.
        assert_eq!(metrics.inner.h, 9);
    }

    #[test]
    fn fully_scrolled_vertical_thumb_reaches_track_bottom_with_h_scrollbar() {
        let root: crate::Element = ScrollView::new()
            .axis(crate::widgets::ScrollAxis::Both)
            .scrollbar(true)
            .h_scrollbar(true)
            .children((0..30).map(|_| {
                Text::new("x".repeat(60))
                    .width(Length::Auto)
                    .height(Length::Px(1))
                    .into()
            }))
            .into();
        let mut tree = reconcile(root, 20, 10);
        let id = tree.root;

        let max_offset = {
            let NodeKind::ScrollView(sv) = &tree.node(id).kind else {
                panic!("expected scroll view");
            };
            sv.max_offset
        };
        assert!(max_offset > 0);
        {
            let NodeKind::ScrollView(sv) = &mut tree.node_mut(id).kind else {
                panic!("expected scroll view");
            };
            sv.offset = max_offset;
            sv.scroll_offset = max_offset as u16;
        }

        let metrics =
            compute_metrics(tree.node(id), ScrollbarAxis::Vertical).expect("vertical metrics");
        // When fully scrolled the thumb must sit flush at the bottom of the track,
        // not half a cell short (which rendered as a `▀` cap). This holds because
        // the metrics' max_offset matches reconcile's, despite the shortened track.
        assert_eq!(
            metrics.core.thumb_start, metrics.core.max_thumb_start,
            "fully-scrolled thumb must reach the track bottom"
        );
    }

    #[test]
    fn scroll_view_vertical_drag_cancels_active_smooth_scroll() {
        let root: crate::Element = ScrollView::new()
            .scrollbar(true)
            .children((0..20).map(|i| Text::new(format!("row {i}")).height(Length::Px(1)).into()))
            .into();
        let mut tree = reconcile(root, 20, 5);
        let root_id = tree.root;
        {
            let NodeKind::ScrollView(scroll) = &mut tree.node_mut(root_id).kind else {
                panic!("expected scroll view");
            };
            scroll
                .smooth_scroll
                .resolve_target(0, 10, scroll.max_offset, linear_smooth());
            assert!(scroll.smooth_scroll.is_animating());
        }

        assert!(handle_drag(
            tree.node_mut(root_id),
            ScrollbarAxis::Vertical,
            0,
            4,
            0,
            0,
        ));

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(
            scroll.offset,
            scroll.smooth_scroll.current_offset(scroll.max_offset)
        );
        assert!(!scroll.smooth_scroll.is_animating());
    }

    #[test]
    fn document_view_vertical_drag_cancels_active_smooth_scroll() {
        let root: crate::Element = DocumentView::new(numbered_lines(20))
            .border(false)
            .wrap(false)
            .scrollbar(true)
            .into();
        let mut tree = reconcile(root, 20, 5);
        let root_id = tree.root;
        {
            let NodeKind::DocumentView(document) = &mut tree.node_mut(root_id).kind else {
                panic!("expected document view");
            };
            document
                .smooth_scroll
                .resolve_target(0, 10, 15, linear_smooth());
            assert!(document.smooth_scroll.is_animating());
        }

        assert!(handle_drag(
            tree.node_mut(root_id),
            ScrollbarAxis::Vertical,
            0,
            4,
            0,
            0,
        ));

        let NodeKind::DocumentView(document) = &tree.node(root_id).kind else {
            panic!("expected document view");
        };
        assert_eq!(
            document.scroll_offset,
            document
                .smooth_scroll
                .current_offset(document.total_visual_lines)
        );
        assert!(!document.smooth_scroll.is_animating());
    }

    #[test]
    fn text_area_vertical_drag_cancels_active_smooth_scroll() {
        let root: crate::Element = TextArea::new(numbered_lines(20)).scrollbar(true).into();
        let mut tree = reconcile(root, 20, 5);
        let root_id = tree.root;
        {
            let NodeKind::TextArea(text_area) = &mut tree.node_mut(root_id).kind else {
                panic!("expected text area");
            };
            text_area
                .smooth_scroll
                .resolve_target(0, 10, 15, linear_smooth());
            assert!(text_area.smooth_scroll.is_animating());
        }

        assert!(handle_drag(
            tree.node_mut(root_id),
            ScrollbarAxis::Vertical,
            0,
            4,
            0,
            0,
        ));

        let NodeKind::TextArea(text_area) = &tree.node(root_id).kind else {
            panic!("expected text area");
        };
        assert_eq!(
            text_area.scroll_offset,
            text_area
                .smooth_scroll
                .current_offset(text_area.visual_lines_count)
        );
        assert!(!text_area.smooth_scroll.is_animating());
    }

    #[test]
    fn text_area_noop_vertical_drag_cancels_active_smooth_scroll() {
        let root: crate::Element = TextArea::new(numbered_lines(20))
            .scrollbar(true)
            .scroll_to_line(10)
            .into();
        let mut tree = reconcile(root, 20, 5);
        let root_id = tree.root;
        {
            let NodeKind::TextArea(text_area) = &mut tree.node_mut(root_id).kind else {
                panic!("expected text area");
            };
            text_area
                .smooth_scroll
                .resolve_target(0, 10, 15, linear_smooth());
            assert!(text_area.smooth_scroll.is_animating());
        }

        assert!(handle_drag(
            tree.node_mut(root_id),
            ScrollbarAxis::Vertical,
            0,
            0,
            0,
            0,
        ));

        let NodeKind::TextArea(text_area) = &tree.node(root_id).kind else {
            panic!("expected text area");
        };
        assert_eq!(text_area.scroll_offset, 0);
        assert_eq!(text_area.cancelled_scroll_to_line, Some(10));
        assert!(!text_area.smooth_scroll.is_animating());
    }

    #[test]
    fn text_area_horizontal_drag_cancels_active_smooth_scroll() {
        let long_lines = (0..10)
            .map(|i| format!("line {i} with enough text to require horizontal scrolling"))
            .collect::<Vec<_>>()
            .join("\n");
        let root: crate::Element = TextArea::new(long_lines)
            .wrap(false)
            .h_scrollbar(true)
            .scroll_to_line(8)
            .into();
        let mut tree = reconcile(root, 20, 5);
        let root_id = tree.root;
        {
            let NodeKind::TextArea(text_area) = &mut tree.node_mut(root_id).kind else {
                panic!("expected text area");
            };
            text_area
                .smooth_scroll
                .resolve_target(0, 8, 15, linear_smooth());
            assert!(text_area.smooth_scroll.is_animating());
        }

        assert!(handle_drag(
            tree.node_mut(root_id),
            ScrollbarAxis::Horizontal,
            0,
            4,
            0,
            0,
        ));

        let NodeKind::TextArea(text_area) = &tree.node(root_id).kind else {
            panic!("expected text area");
        };
        assert_eq!(text_area.cancelled_scroll_to_line, Some(8));
        assert!(!text_area.smooth_scroll.is_animating());
    }

    #[test]
    fn document_view_horizontal_drag_cancels_active_smooth_scroll() {
        let long_lines = (0..10)
            .map(|i| format!("line {i} with enough text to require horizontal scrolling"))
            .collect::<Vec<_>>()
            .join("\n");
        let root: crate::Element = DocumentView::new(long_lines)
            .border(false)
            .wrap(false)
            .h_scrollbar(true)
            .scroll_to_source_line(8)
            .into();
        let mut tree = reconcile(root, 20, 5);
        let root_id = tree.root;
        {
            let NodeKind::DocumentView(document) = &mut tree.node_mut(root_id).kind else {
                panic!("expected document view");
            };
            document
                .smooth_scroll
                .resolve_target(0, 8, 15, linear_smooth());
            assert!(document.smooth_scroll.is_animating());
        }

        assert!(handle_drag(
            tree.node_mut(root_id),
            ScrollbarAxis::Horizontal,
            0,
            4,
            0,
            0,
        ));

        let NodeKind::DocumentView(document) = &tree.node(root_id).kind else {
            panic!("expected document view");
        };
        assert_eq!(document.cancelled_scroll_to_source_line, Some(8));
        assert!(!document.smooth_scroll.is_animating());
    }
}
