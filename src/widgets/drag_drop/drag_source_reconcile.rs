use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{
    OverlayState, ReconcileCtx, SingleChildReconcile, reconcile_single_child,
};
use crate::style::Rect;

use super::drag_source::DragSource;
use super::drag_source_node::DragSourceNode;

pub(crate) fn reconcile_drag_source(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    source: &DragSource,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        let is_dragging = if let NodeKind::DragSource(existing) = &node.kind {
            existing.is_dragging
        } else {
            false
        };
        node.rect = rect;
        node.kind = NodeKind::DragSource(DragSourceNode {
            on_drag_start: source.on_drag_start.clone(),
            on_drag_cancel: source.on_drag_cancel.clone(),
            on_drag_started: source.on_drag_started.clone(),
            drag_group: source.drag_group.clone(),
            preview: source.preview.clone(),
            dragging_style: source.dragging_style,
            drag_slot: source.drag_slot,
            preview_max_width: source.preview_max_width,
            preview_max_height: source.preview_max_height,
            threshold: source.threshold,
            enabled: source.enabled,
            is_dragging,
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
            child: source.child.as_deref(),
            rect,
            old_children,
        },
    );

    let node = tree.node_mut(id);
    node.children = new_children;

    id
}
