use std::collections::HashSet;
use std::ops::Range;

use ratatui::buffer::{Buffer, Cell};

use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{Rect, ScrollbarVariant};

use super::ScrollFrameSnapshot;

#[derive(Clone, Debug)]
pub(super) struct IncrementalScrollPlan {
    pub(super) scroll_rows: Range<u16>,
    pub(super) delta_rows: i16,
    pub(super) repaint_regions: Vec<Rect>,
}
pub(super) fn collect_descendants(tree: &NodeTree, root: NodeId, out: &mut HashSet<NodeId>) {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !tree.is_valid(id) || !out.insert(id) {
            continue;
        }
        for &child in &tree.node(id).children {
            stack.push(child);
        }
    }
}

pub(super) fn subtree_has_hoverables(tree: &NodeTree, root: NodeId) -> bool {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !tree.is_valid(id) {
            continue;
        }

        let node = tree.node(id);
        if node.is_hoverable() {
            return true;
        }

        stack.extend(node.children.iter().copied());
    }

    false
}

pub(super) fn collect_scroll_repaint_regions(
    tree: &NodeTree,
    scroll_id: NodeId,
    scroll_band: Rect,
    frame_area: ratatui::layout::Rect,
) -> Vec<Rect> {
    let mut allowed = HashSet::new();
    collect_descendants(tree, scroll_id, &mut allowed);

    let mut parent = Some(scroll_id);
    while let Some(id) = parent {
        if !tree.is_valid(id) {
            break;
        }
        allowed.insert(id);
        parent = tree.node(id).parent;
    }

    let mut regions = Vec::new();

    for node in tree.iter() {
        if allowed.contains(&node.id) {
            continue;
        }

        if let Some(parent) = node.parent
            && !allowed.contains(&parent)
        {
            let parent_rect = tree.node(parent).rect.intersection(&scroll_band);
            if !parent_rect.is_empty() {
                continue;
            }
        }

        let clipped = node.rect.intersection(&scroll_band);
        if clipped.is_empty() {
            continue;
        }

        regions.push(Rect {
            x: clipped.x.saturating_add(frame_area.x as i16),
            y: clipped.y.saturating_add(frame_area.y as i16),
            w: clipped.w,
            h: clipped.h,
        });
    }

    regions
}

pub(super) fn capture_scroll_frames(tree: &NodeTree) -> Vec<ScrollFrameSnapshot> {
    let mut snapshots = Vec::new();

    for node in tree.iter() {
        let NodeKind::ScrollView(scroll_view) = &node.kind else {
            continue;
        };

        let mut inner = node
            .rect
            .inner(scroll_view.props.border, scroll_view.props.padding);
        if inner.w == 0 || inner.h == 0 {
            continue;
        }

        if scroll_view.show_scroll_indicators {
            if scroll_view.top_indicator {
                inner.y = inner.y.saturating_add(1);
                inner.h = inner.h.saturating_sub(1);
            }
            if scroll_view.bottom_indicator {
                inner.h = inner.h.saturating_sub(1);
            }
        }

        // Exclude the standalone horizontal scrollbar row from the scrollable
        // region (mirrors the renderer). Otherwise the incremental scroll-region
        // shift drags the bottom scrollbar with the content, leaving trails and
        // duplicates during vertical scrolling.
        let h_integrated = scroll_view.props.border
            && scroll_view.h_scrollbar
            && matches!(
                scroll_view.h_scrollbar_variant,
                ScrollbarVariant::Integrated
            );
        let h_standalone = scroll_view.h_scrollbar && scroll_view.h_max_offset > 0 && !h_integrated;
        if h_standalone && inner.h > 0 {
            inner.h = inner
                .h
                .saturating_sub(1u16.saturating_add(scroll_view.h_scrollbar_gap));
        }

        if inner.h == 0 {
            continue;
        }

        let parent_border_x = if !scroll_view.props.border
            && scroll_view.scrollbar
            && matches!(scroll_view.scrollbar_variant, ScrollbarVariant::Integrated)
        {
            tree.ancestor_frame_integrated_vscrollbar_x(node.parent)
        } else {
            None
        };

        let use_integrated = (scroll_view.props.border || parent_border_x.is_some())
            && matches!(scroll_view.scrollbar_variant, ScrollbarVariant::Integrated);
        let use_standalone = scroll_view.scrollbar && !use_integrated;

        let scrollbar_rect = if scroll_view.scrollbar && inner.w > 0 && inner.h > 0 {
            let x = if use_integrated {
                parent_border_x.unwrap_or_else(|| {
                    node.rect
                        .x
                        .saturating_add(node.rect.w.saturating_sub(1) as i16)
                })
            } else if use_standalone {
                inner.x.saturating_add(inner.w.saturating_sub(1) as i16)
            } else {
                0
            };

            if use_integrated || use_standalone {
                Some(Rect {
                    x,
                    y: inner.y,
                    w: 1,
                    h: inner.h,
                })
            } else {
                None
            }
        } else {
            None
        };

        snapshots.push(ScrollFrameSnapshot {
            node_id: node.id,
            scroll_offset: scroll_view.scroll_offset,
            content_height: scroll_view.content_height,
            content_hash: scroll_view.layout_cache.active_content_hash,
            viewport_height: scroll_view.viewport_height,
            scroll_rows: inner.y.max(0) as u16
                ..inner.y.saturating_add(inner.h as i16).max(0) as u16,
            scrollbar_rect,
            show_scroll_indicators: scroll_view.show_scroll_indicators,
        });
    }

    snapshots
}

pub(super) fn shift_buffer_rows(buf: &mut Buffer, rows: &Range<u16>, delta_rows: i16) {
    if delta_rows == 0 || rows.start >= rows.end {
        return;
    }

    let area = buf.area;
    let start = rows.start.saturating_sub(area.y) as usize;
    let end = rows.end.saturating_sub(area.y) as usize;
    if start >= end || end > area.height as usize {
        return;
    }

    let height = end - start;
    let shift = delta_rows.unsigned_abs() as usize;
    if shift == 0 || shift >= height {
        return;
    }

    let width = area.width as usize;

    if delta_rows > 0 {
        for row in 0..(height - shift) {
            let dst = (start + row) * width;
            let src = (start + row + shift) * width;
            let (head, tail) = buf.content.split_at_mut(src);
            head[dst..dst + width].clone_from_slice(&tail[..width]);
        }
    } else {
        for row in (shift..height).rev() {
            let dst = (start + row) * width;
            let src = (start + row - shift) * width;
            let (head, tail) = buf.content.split_at_mut(dst);
            tail[..width].clone_from_slice(&head[src..src + width]);
        }
    }
}

pub(super) fn clear_buffer_rows(buf: &mut Buffer, rows: &Range<u16>) {
    if rows.start >= rows.end {
        return;
    }

    let area = buf.area;
    let start = rows.start.saturating_sub(area.y) as usize;
    let end = rows.end.saturating_sub(area.y) as usize;
    if start >= end || end > area.height as usize {
        return;
    }

    // Rows exposed by a scroll must be cleared to the configured root viewport
    // background (if any), not to blank cells, so the screen background stays
    // continuous through scroll regions on the incremental draw fast path.
    let fill_cell =
        match crate::backend::ratatui_backend::common::current_render_screen_background() {
            Some(style) => {
                let mut cell = Cell::EMPTY;
                cell.set_style(style);
                cell
            }
            None => Cell::EMPTY,
        };

    let width = area.width as usize;
    for row in start..end {
        let row_start = row * width;
        let row_end = row_start + width;
        buf.content[row_start..row_end].fill(fill_cell.clone());
    }
}

pub(super) fn replace_buffer_snapshot(slot: &mut Option<Buffer>, source: &Buffer) {
    match slot {
        Some(snapshot) if snapshot.area == source.area => {
            snapshot.content.clone_from(&source.content);
        }
        Some(snapshot) => {
            snapshot.resize(source.area);
            snapshot.content.clone_from(&source.content);
        }
        None => *slot = Some(source.clone()),
    }
}
