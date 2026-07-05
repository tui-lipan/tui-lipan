use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::{ElementReconcile, OverlayState, ReconcileCtx, reconcile_element};
use crate::layout::tag::can_reuse;
use crate::style::{Align, Rect};
use crate::widgets::Frame;

use super::box_metrics::{FrameJoinOverlap, compute_frame_geometry};

fn rect_right(rect: Rect) -> i16 {
    rect.x.saturating_add(rect.w as i16).saturating_sub(1)
}

fn rect_bottom(rect: Rect) -> i16 {
    rect.y.saturating_add(rect.h as i16).saturating_sub(1)
}

fn spans_overlap(a_start: i16, a_end: i16, b_start: i16, b_end: i16) -> bool {
    a_start <= b_end && b_start <= a_end
}

fn frame_join_overlap(tree: &NodeTree, id: NodeId, frame: &Frame) -> (bool, bool) {
    if !frame.props.join_frame || !frame.props.has_border() {
        return (false, false);
    }

    let node = tree.node(id);
    let Some(parent_id) = node.parent else {
        return (false, false);
    };

    let self_rect = node.rect.inset(frame.props.decoration_outside_padding());
    if self_rect.is_empty() {
        return (false, false);
    }

    let self_left = self_rect.x;
    let self_top = self_rect.y;
    let self_right = rect_right(self_rect);
    let self_bottom = rect_bottom(self_rect);

    let mut join_left = false;
    let mut join_top = false;

    let parent = tree.node(parent_id);
    for &sibling_id in &parent.children {
        if sibling_id == id || !tree.is_valid(sibling_id) {
            continue;
        }

        let sibling = tree.node(sibling_id);
        let sibling_rect = match &sibling.kind {
            NodeKind::Frame(sibling_props) => {
                if !sibling_props.join_frame || !sibling_props.has_border() {
                    continue;
                }
                let r = sibling
                    .rect
                    .inset(sibling_props.decoration_outside_padding());
                if r.is_empty() {
                    continue;
                }
                r
            }
            NodeKind::HStack(_) | NodeKind::VStack(_) | NodeKind::Grid(_) | NodeKind::Flow(_) => {
                // Frame can join with a stack when adjacent (e.g. caption Frame below
                // HStack of image Frames). Use the stack's rect directly.
                if sibling.rect.is_empty() {
                    continue;
                }
                sibling.rect
            }
            _ => continue,
        };

        let sibling_left = sibling_rect.x;
        let sibling_top = sibling_rect.y;
        let sibling_right = rect_right(sibling_rect);
        let sibling_bottom = rect_bottom(sibling_rect);

        if sibling_right.saturating_add(1) == self_left
            && spans_overlap(sibling_top, sibling_bottom, self_top, self_bottom)
        {
            join_left = true;
        }

        if sibling_bottom.saturating_add(1) == self_top
            && spans_overlap(sibling_left, sibling_right, self_left, self_right)
        {
            join_top = true;
        }

        if join_left && join_top {
            break;
        }
    }

    (join_left, join_top)
}

pub(crate) fn reconcile_frame(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    frame: &Frame,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::Frame(frame.props.clone());
        std::mem::take(&mut node.children)
    };

    let has_parent = tree.node(id).parent.is_some();
    let (join_left, join_top) = frame_join_overlap(tree, id, frame);
    let join_overlap = FrameJoinOverlap {
        left: join_left,
        top: join_top,
    };
    let geometry = compute_frame_geometry(&frame.props, rect, join_overlap, has_parent);
    let content = geometry.content_rect;
    let header_rect = geometry.header_rect;

    let reuse_header = frame.header.as_deref().and_then(|header| {
        old_children
            .iter()
            .copied()
            .find(|id| tree.is_valid(*id) && can_reuse(tree.node(*id), header))
    });
    let reuse_child = frame.child.as_deref().and_then(|child| {
        old_children.iter().copied().find(|id| {
            tree.is_valid(*id) && Some(*id) != reuse_header && can_reuse(tree.node(*id), child)
        })
    });

    let mut new_children = old_children;
    new_children.clear();

    if let (Some(header), Some(rect)) = (frame.header.as_deref(), header_rect) {
        let header_id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: reuse_header,
                parent: Some(id),
                el: header,
                rect,
            },
        );
        new_children.push(header_id);
    }

    if let Some(child) = frame.child.as_deref() {
        let child_rect = match frame.props.child_align {
            Align::Start | Align::Stretch => content,
            Align::Center => {
                let (child_w, child_h) =
                    min_size_constrained(child, Some(content.w), Some(content.h));
                let w = child_w.min(content.w);
                let h = child_h.min(content.h);
                let x = content
                    .x
                    .saturating_add((content.w.saturating_sub(w) / 2) as i16);
                let y = content
                    .y
                    .saturating_add((content.h.saturating_sub(h) / 2) as i16);
                Rect { x, y, w, h }
            }
            Align::End => {
                let (child_w, child_h) =
                    min_size_constrained(child, Some(content.w), Some(content.h));
                let w = child_w.min(content.w);
                let h = child_h.min(content.h);
                let x = content.x.saturating_add(content.w.saturating_sub(w) as i16);
                let y = content.y.saturating_add(content.h.saturating_sub(h) as i16);
                Rect { x, y, w, h }
            }
        };
        let child_id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: reuse_child,
                parent: Some(id),
                el: child,
                rect: child_rect,
            },
        );
        new_children.push(child_id);
    }

    let node = tree.node_mut(id);
    node.children = new_children;

    id
}
