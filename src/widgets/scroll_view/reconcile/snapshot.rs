use std::collections::{HashMap, HashSet};

use crate::core::component::FocusContext;
use crate::core::element::{Element, ElementKind, Key};
use crate::core::node::{NodeId, NodeTree};
use crate::layout::reconcile::{ElementReconcile, OverlayState, ReconcileCtx, reconcile_element};
use crate::layout::stack::{ScrollContentLayout, position_scroll_content_rects};
use crate::style::Rect;
use crate::widgets::containers::reconcile::stack_reuse_plan;
use crate::widgets::scroll_view::node::OffscreenDocSelection;
use crate::widgets::scroll_view::node::ScrollViewportSnapshot;
use crate::widgets::{
    ScrollChildExitDirection, ScrollChildVisibility, ScrollExitedChild, ScrollMetrics,
    ScrollViewportEvent, ScrollVisibleChild,
};

pub(crate) fn recompute_scroll_content_height_with_reconciled_roots(
    children: &[Element],
    content_layout: &ScrollContentLayout,
    visible_indices: &[usize],
    visible_rects: &[Rect],
    reconciled_ids: &[NodeId],
    tree: &NodeTree,
    props_gap: u16,
) -> ScrollContentLayout {
    let mut rects = content_layout.rects.clone();
    if !children
        .iter()
        .any(|c| !matches!(c.kind, ElementKind::Portal(_)))
        || rects.is_empty()
    {
        return crate::layout::stack::make_scroll_content_layout(rects, 0);
    }

    let mut actual_h: Vec<Option<u16>> = vec![None; children.len()];
    for (slot, &idx) in visible_indices.iter().enumerate() {
        if let Some(&cid) = reconciled_ids.get(slot)
            && idx < actual_h.len()
            && tree.is_valid(cid)
            && content_layout
                .rects
                .get(idx)
                .zip(visible_rects.get(slot))
                .is_some_and(|(content_rect, visible_rect)| visible_rect.h >= content_rect.h)
        {
            actual_h[idx] = Some(tree.node(cid).rect.h);
        }
    }

    for (i, rect) in rects.iter_mut().enumerate() {
        if let Some(ah) = actual_h.get(i).copied().flatten() {
            rect.h = ah;
        }
    }

    let content_height = position_scroll_content_rects(children, &mut rects, props_gap);
    crate::layout::stack::make_scroll_content_layout(rects, content_height)
}

pub(crate) struct ScrollVisibleCollectCtx {
    pub inner: Rect,
    pub viewport_w: u16,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub effective_offset: usize,
    pub h_offset: usize,
}

pub(crate) fn collect_visible_scroll_children(
    children: &[Element],
    content_layout: &ScrollContentLayout,
    ctx: ScrollVisibleCollectCtx,
) -> (Vec<usize>, Vec<Rect>) {
    let ScrollVisibleCollectCtx {
        inner,
        viewport_w,
        top_indicator,
        bottom_indicator,
        effective_offset,
        h_offset,
    } = ctx;
    let mut visible_viewport = Rect {
        w: viewport_w,
        ..inner
    };
    if top_indicator {
        visible_viewport.y = visible_viewport.y.saturating_add(1);
        visible_viewport.h = visible_viewport.h.saturating_sub(1);
    }
    if bottom_indicator {
        visible_viewport.h = visible_viewport.h.saturating_sub(1);
    }

    let range_start = effective_offset as u16;
    let range_end = range_start.saturating_add(visible_viewport.h);
    let mut visible_indices = Vec::new();
    let mut visible_rects = Vec::with_capacity(children.len());

    for (i, r) in content_layout.rects.iter().enumerate() {
        if r.w == 0 || r.h == 0 {
            continue;
        }

        let child_bottom = r.y.saturating_add(r.h as i16);
        if child_bottom > range_start as i16 && r.y < range_end as i16 {
            visible_indices.push(i);

            let x = inner.x.saturating_add(r.x).saturating_sub(h_offset as i16);
            let y = (visible_viewport.y as i32 + r.y as i32 - range_start as i32) as i16;

            visible_rects.push(Rect {
                x,
                y,
                w: r.w,
                h: r.h,
            });
        }
    }

    (visible_indices, visible_rects)
}

fn same_scroll_child_identity(a: &ScrollVisibleChild, b: &ScrollVisibleChild) -> bool {
    match (a.key.as_ref(), b.key.as_ref()) {
        (Some(a_key), Some(b_key)) => a_key == b_key,
        (None, None) => a.index == b.index,
        _ => false,
    }
}

pub(crate) struct ScrollSnapshotBuildCtx {
    pub effective_offset: usize,
    pub visible_rows: usize,
    pub viewport_w: u16,
    pub content_height: u16,
    pub max_offset: usize,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
}

pub(crate) fn build_scroll_viewport_snapshot(
    children: &[Element],
    content_layout: &ScrollContentLayout,
    visible_indices: &[usize],
    ctx: ScrollSnapshotBuildCtx,
) -> ScrollViewportSnapshot {
    let ScrollSnapshotBuildCtx {
        effective_offset,
        visible_rows,
        viewport_w,
        content_height,
        max_offset,
        top_indicator,
        bottom_indicator,
        bottom_count,
    } = ctx;
    let viewport_h = visible_rows.min(u16::MAX as usize) as u16;
    let mut visible = Vec::with_capacity(visible_indices.len());
    let offset_i32 = effective_offset.min(i32::MAX as usize) as i32;

    for &idx in visible_indices {
        let Some(content_rect) = content_layout.rects.get(idx).copied() else {
            continue;
        };
        if content_rect.w == 0 || content_rect.h == 0 {
            continue;
        }

        let viewport_y_i32 = i32::from(content_rect.y).saturating_sub(offset_i32);
        let viewport_rect = Rect {
            x: content_rect.x,
            y: viewport_y_i32.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            w: content_rect.w,
            h: content_rect.h,
        };
        let viewport_bounds = Rect {
            x: 0,
            y: 0,
            w: viewport_w,
            h: viewport_h,
        };
        let visible_rect = viewport_rect.intersection(&viewport_bounds);
        if visible_rect.is_empty() {
            continue;
        }

        let child_top = viewport_y_i32;
        let child_bottom = child_top.saturating_add(i32::from(content_rect.h));
        let clipped_above = 0i32
            .saturating_sub(child_top)
            .max(0)
            .min(i32::from(content_rect.h)) as u16;
        let clipped_below = child_bottom
            .saturating_sub(i32::from(viewport_h))
            .max(0)
            .min(i32::from(content_rect.h)) as u16;
        let visible_height = visible_rect.h;
        let visibility = if visible_rect == viewport_rect {
            ScrollChildVisibility::FullyVisible
        } else {
            ScrollChildVisibility::PartiallyVisible
        };

        visible.push(ScrollVisibleChild {
            index: idx,
            key: children.get(idx).and_then(|child| child.key.clone()),
            content_rect,
            viewport_rect,
            visible_rect,
            visible_height,
            clipped_above,
            clipped_below,
            visibility,
        });
    }

    ScrollViewportSnapshot {
        offset: effective_offset,
        metrics: ScrollMetrics {
            len: content_height as usize,
            visible: visible_rows,
            max_offset,
        },
        viewport_width: viewport_w,
        children_len: children.len(),
        first_visible_index: visible.first().map(|child| child.index),
        last_visible_index: visible.last().map(|child| child.index),
        visible,
        top_indicator,
        bottom_indicator,
        bottom_count,
    }
}

fn find_current_child_rect_by_identity(
    children: &[Element],
    rects: &[Rect],
    child: &ScrollVisibleChild,
) -> Option<Rect> {
    if let Some(key) = child.key.as_ref() {
        children
            .iter()
            .position(|candidate| candidate.key.as_ref() == Some(key))
            .and_then(|idx| rects.get(idx).copied())
    } else {
        rects.get(child.index).copied()
    }
}

fn classify_exited_scroll_child(
    children: &[Element],
    content_layout: &ScrollContentLayout,
    effective_offset: usize,
    visible_rows: usize,
    child: &ScrollVisibleChild,
) -> ScrollChildExitDirection {
    let Some(rect) = find_current_child_rect_by_identity(children, &content_layout.rects, child)
    else {
        return ScrollChildExitDirection::Removed;
    };
    if rect.w == 0 || rect.h == 0 {
        return ScrollChildExitDirection::Removed;
    }

    let viewport_start = effective_offset.min(i32::MAX as usize) as i32;
    let viewport_end = viewport_start.saturating_add(visible_rows.min(i32::MAX as usize) as i32);
    let child_top = i32::from(rect.y);
    let child_bottom = child_top.saturating_add(i32::from(rect.h));
    if child_bottom <= viewport_start {
        ScrollChildExitDirection::Above
    } else if child_top >= viewport_end {
        ScrollChildExitDirection::Below
    } else {
        ScrollChildExitDirection::Removed
    }
}

pub(crate) fn build_scroll_viewport_event(
    snapshot: &ScrollViewportSnapshot,
    previous: Option<&ScrollViewportSnapshot>,
    children: &[Element],
    content_layout: &ScrollContentLayout,
) -> ScrollViewportEvent {
    let entered = snapshot
        .visible
        .iter()
        .filter(|child| {
            previous.is_none_or(|prev| {
                !prev
                    .visible
                    .iter()
                    .any(|old| same_scroll_child_identity(old, child))
            })
        })
        .cloned()
        .collect();

    let exited = previous
        .map(|prev| {
            prev.visible
                .iter()
                .filter(|old| {
                    !snapshot
                        .visible
                        .iter()
                        .any(|child| same_scroll_child_identity(old, child))
                })
                .map(|old| ScrollExitedChild {
                    child: old.clone(),
                    direction: classify_exited_scroll_child(
                        children,
                        content_layout,
                        snapshot.offset,
                        snapshot.metrics.visible,
                        old,
                    ),
                })
                .collect()
        })
        .unwrap_or_default();

    ScrollViewportEvent {
        offset: snapshot.offset,
        metrics: snapshot.metrics,
        viewport_width: snapshot.viewport_width,
        children_len: snapshot.children_len,
        first_visible_index: snapshot.first_visible_index,
        last_visible_index: snapshot.last_visible_index,
        visible: snapshot.visible.clone(),
        entered,
        exited,
        top_indicator: snapshot.top_indicator,
        bottom_indicator: snapshot.bottom_indicator,
        bottom_count: snapshot.bottom_count,
    }
}

pub(crate) fn take_visible_doc_restores(
    children: &[Element],
    visible_indices: &[usize],
    offscreen_doc_selections: &mut HashMap<Key, OffscreenDocSelection>,
    visible_doc_restores: &mut HashMap<Key, OffscreenDocSelection>,
) -> HashSet<Key> {
    let visible_child_keys: HashSet<Key> = visible_indices
        .iter()
        .filter_map(|&idx| children.get(idx).and_then(|child| child.key.clone()))
        .collect();
    for key in &visible_child_keys {
        if let Some(saved) = offscreen_doc_selections.remove(key) {
            visible_doc_restores.insert(key.clone(), saved);
        }
    }
    visible_child_keys
}

pub(crate) struct ScrollVisibleReconcileCtx<'a> {
    pub epoch: u32,
    pub parent_id: NodeId,
    pub old_children: Vec<NodeId>,
    pub visible_doc_restores: &'a mut HashMap<Key, OffscreenDocSelection>,
    pub focus: Option<&'a FocusContext>,
    pub overlay_state: &'a mut OverlayState,
}

pub(crate) fn reconcile_visible_scroll_children(
    tree: &mut NodeTree,
    children: &[Element],
    visible_indices: &[usize],
    visible_rects: &[Rect],
    ctx: ScrollVisibleReconcileCtx<'_>,
) -> Vec<NodeId> {
    let ScrollVisibleReconcileCtx {
        epoch,
        parent_id,
        old_children,
        visible_doc_restores,
        focus,
        overlay_state,
    } = ctx;
    let visible_children: Vec<_> = visible_indices.iter().map(|&i| &children[i]).collect();
    let plan = stack_reuse_plan(tree, &old_children, &visible_children);

    let mut new_children = Vec::with_capacity(visible_children.len());
    for ((child, reuse_id), child_rect) in
        visible_children.iter().zip(plan).zip(visible_rects.iter())
    {
        let restore_docs = child
            .key
            .as_ref()
            .and_then(|key| visible_doc_restores.remove(key))
            .map(|saved| saved.docs);
        let has_restore_docs = restore_docs.is_some();
        if let Some(docs) = restore_docs {
            tree.push_offscreen_doc_restore(docs);
        }
        let child_id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: reuse_id,
                parent: Some(parent_id),
                el: child,
                rect: *child_rect,
            },
        );
        if has_restore_docs {
            tree.pop_offscreen_doc_restore();
        }
        new_children.push(child_id);
    }

    new_children
}
