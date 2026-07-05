use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::axis::resolve_size;
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::{
    OverlayState, ReconcileCtx, SingleChildReconcile, reconcile_single_child,
};
use crate::style::Rect;

pub(crate) fn reconcile_center(
    tree: &mut NodeTree,
    id: NodeId,
    center: &super::Center,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
    epoch: u32,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::Center(super::CenterNode {
            style: center.style,
        });
        std::mem::take(&mut node.children)
    };

    let child_rect = center.child.as_deref().map(|child| {
        let (min_w, min_h) = min_size_constrained(child, Some(rect.w), Some(rect.h));

        let w = resolve_size(center.width, rect.w, min_w);
        let h = resolve_size(center.height, rect.h, min_h);

        let x = rect.x.saturating_add((rect.w.saturating_sub(w) / 2) as i16);
        let y = rect.y.saturating_add((rect.h.saturating_sub(h) / 2) as i16);

        Rect { x, y, w, h }
    });

    let new_children = reconcile_single_child(
        &mut ReconcileCtx {
            tree,
            epoch,
            focus,
            overlay_state,
        },
        SingleChildReconcile {
            parent_id: id,
            child: center.child.as_deref(),
            rect: child_rect.unwrap_or(rect),
            old_children,
        },
    );

    let node = tree.node_mut(id);
    node.children = new_children;

    id
}
