use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{
    OverlayState, ReconcileCtx, SingleChildReconcile, reconcile_single_child,
};
use crate::style::Rect;

use super::{MouseRegion, MouseRegionNode};

pub(crate) fn reconcile_mouse_region(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    region: &MouseRegion,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::MouseRegion(MouseRegionNode {
            on_click: region.on_click.clone(),
            on_mouse_down: region.on_mouse_down.clone(),
            bubble_mouse_down: region.bubble_mouse_down,
            on_mouse_up: region.on_mouse_up.clone(),
            on_mouse_move: region.on_mouse_move.clone(),
            on_drag_start: region.on_drag_start.clone(),
            on_drag: region.on_drag.clone(),
            on_drag_end: region.on_drag_end.clone(),
            drag_required_mods: region.drag_required_mods,
            on_right_drag_start: region.on_right_drag_start.clone(),
            on_right_drag: region.on_right_drag.clone(),
            on_right_drag_end: region.on_right_drag_end.clone(),
            right_drag_required_mods: region.right_drag_required_mods,
            on_hover_change: region.on_hover_change.clone(),
            hit_test: region.hit_test.clone(),
            capture_click: region.capture_click,
            capture_required_mods: region.capture_required_mods,
            hover_style: region.hover_style,
            hover_effects: region.hover_effects.clone(),
            enabled: region.enabled,
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
            child: region.child.as_deref(),
            rect,
            old_children,
        },
    );

    let node = tree.node_mut(id);
    node.children = new_children;

    id
}
