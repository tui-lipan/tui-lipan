use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{
    OverlayState, ReconcileCtx, SingleChildReconcile, reconcile_single_child,
};
use crate::style::Rect;

use super::drop_target::DropTarget;
use super::drop_target_node::DropTargetNode;

pub(crate) fn reconcile_drop_target(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    target: &DropTarget,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        let dnd_highlighted = if let NodeKind::DropTarget(existing) = &node.kind {
            existing.dnd_highlighted
        } else {
            false
        };
        node.rect = rect;
        node.kind = NodeKind::DropTarget(DropTargetNode {
            on_drag_over: target.on_drag_over.clone(),
            on_drag_leave: target.on_drag_leave.clone(),
            on_drop: target.on_drop.clone(),
            accept_group: target.accept_group.clone(),
            can_accept: target.can_accept.clone(),
            highlight: target.highlight,
            highlight_style: target.highlight_style,
            drop_slot: target.drop_slot,
            enabled: target.enabled,
            dnd_highlighted,
        });
        std::mem::take(&mut node.children)
    };

    let new_children = reconcile_single_child(
        &mut ReconcileCtx {
            tree,
            epoch,
            focus,
            overlay_state,
        },
        SingleChildReconcile {
            parent_id: id,
            child: target.child.as_deref(),
            rect,
            old_children,
        },
    );

    let node = tree.node_mut(id);
    node.children = new_children;

    id
}
