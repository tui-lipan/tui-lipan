use super::layout::measure_center_child;
use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{ElementReconcile, OverlayState, ReconcileCtx, reconcile_element};
use crate::layout::tag::can_reuse;
use crate::style::Rect;

pub(crate) fn reconcile_center_pin(
    tree: &mut NodeTree,
    id: NodeId,
    cp: &super::CenterPin,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
    epoch: u32,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::CenterPin(super::CenterPinNode { style: cp.style });
        std::mem::take(&mut node.children)
    };

    // --- Measure and position the center child ---------------------------------
    //
    // The center child is measured first so its height determines where the
    // top/bottom zones start and end.  Both zones receive the space that remains
    // on either side of the pinned child.

    let (center_w, center_h) = cp
        .center
        .as_deref()
        .map(|c| measure_center_child(c, Some(rect.w), Some(rect.h)))
        .unwrap_or((0, 0));

    // Clamp to available space so the center rect never exceeds the container
    // and all subsequent i16 arithmetic stays in bounds.
    let center_h = center_h.min(rect.h);
    let center_w = center_w.min(rect.w);

    // Split remaining vertical space between top and bottom zones.
    // The remainder (odd heights) goes to bottom so the center child sits
    // slightly above true mathematical center - consistent with common UI
    // conventions (e.g. modal dialogs are perceived as centered when placed
    // at the upper half of the remaining space).
    let remaining_v = rect.h.saturating_sub(center_h);
    let top_h = remaining_v / 2;
    let bottom_h = remaining_v / 2 + remaining_v % 2;

    let center_y = rect.y.saturating_add(top_h as i16);
    let center_x = rect
        .x
        .saturating_add((rect.w.saturating_sub(center_w) / 2) as i16);

    let top_rect = Rect {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: top_h,
    };
    let center_rect = Rect {
        x: center_x,
        y: center_y,
        w: center_w,
        h: center_h,
    };
    let bottom_rect = Rect {
        x: rect.x,
        y: center_y.saturating_add(center_h as i16),
        w: rect.w,
        h: bottom_h,
    };

    // --- Reuse helpers ---------------------------------------------------------

    // Keep slot reuse one-to-one. Without this, multiple slots with the same
    // unkeyed tag (e.g. center=VStack and bottom=VStack) can pick the same old
    // node id, causing one slot to overwrite the other on the next frame.
    let mut claimed_reuse_ids = Vec::with_capacity(3);
    let mut find_reuse = |el: &crate::core::element::Element| {
        let reuse_id = old_children.iter().copied().find(|cid| {
            tree.is_valid(*cid)
                && !claimed_reuse_ids.contains(cid)
                && can_reuse(tree.node(*cid), el)
        });
        if let Some(id) = reuse_id {
            claimed_reuse_ids.push(id);
        }
        reuse_id
    };

    let reuse_top = cp.top.as_deref().and_then(&mut find_reuse);
    let reuse_center = cp.center.as_deref().and_then(&mut find_reuse);
    let reuse_bottom = cp.bottom.as_deref().and_then(&mut find_reuse);

    let mut new_children = old_children;
    new_children.clear();

    // --- Reconcile children ----------------------------------------------------
    //
    // Render order: top → center → bottom so that center paints over the top
    // zone and bottom over the center in the unlikely case of overflow.

    if let Some(top) = cp.top.as_deref() {
        let child_id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: reuse_top,
                parent: Some(id),
                el: top,
                rect: top_rect,
            },
        );
        new_children.push(child_id);
    }

    if let Some(center) = cp.center.as_deref() {
        let child_id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: reuse_center,
                parent: Some(id),
                el: center,
                rect: center_rect,
            },
        );
        new_children.push(child_id);
    }

    if let Some(bottom) = cp.bottom.as_deref() {
        let child_id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: reuse_bottom,
                parent: Some(id),
                el: bottom,
                rect: bottom_rect,
            },
        );
        new_children.push(child_id);
    }

    let node = tree.node_mut(id);
    node.children = new_children;

    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::node::NodeTree;
    use crate::layout::reconcile::reconcile_with_overlays_mode;
    use crate::widgets::{CenterPin, Text, VStack};

    fn center_pin_with_same_tag_slots() -> crate::core::element::Element {
        CenterPin::new()
            .center(VStack::new().child(Text::new("center")))
            .bottom(VStack::new().child(Text::new("bottom")))
            .into()
    }

    #[test]
    fn center_and_bottom_reuse_distinct_node_ids_when_same_tag() {
        let root = center_pin_with_same_tag_slots();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };
        let mut tree = NodeTree::new();

        // First pass allocates nodes; second pass must reuse them one-to-one.
        // Regression: both slots reused the same old VStack id, so center content
        // disappeared and the tree became structurally invalid.
        reconcile_with_overlays_mode(&mut tree, &root, bounds, None, &[], false);
        reconcile_with_overlays_mode(&mut tree, &root, bounds, None, &[], false);

        let root_node = tree.node(tree.root);
        assert_eq!(
            root_node.children.len(),
            2,
            "expected center and bottom children"
        );
        assert_ne!(
            root_node.children[0], root_node.children[1],
            "center and bottom must not reuse the same node id"
        );

        let center_rect = tree.node(root_node.children[0]).rect;
        let bottom_rect = tree.node(root_node.children[1]).rect;
        assert!(
            center_rect.y < bottom_rect.y,
            "center child should be above bottom child (center_y={}, bottom_y={})",
            center_rect.y,
            bottom_rect.y
        );
    }
}
