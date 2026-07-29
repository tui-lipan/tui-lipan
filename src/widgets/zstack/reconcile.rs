use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{ElementReconcile, OverlayState, ReconcileCtx, reconcile_element};
use crate::style::Rect;
use crate::widgets::containers::exit_retention;
use crate::widgets::containers::reconcile::stack_reuse_plan;

pub(crate) fn reconcile_zstack(
    tree: &mut NodeTree,
    id: NodeId,
    zstack: &super::ZStack,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
    epoch: u32,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        node.rect = rect;
        // Replacing the kind wholesale would drop the retention list, which must survive
        // every frame an exit animation is still running.
        let exiting = match &mut node.kind {
            NodeKind::ZStack(zstack) => std::mem::take(&mut zstack.exiting),
            _ => Vec::new(),
        };
        node.kind = NodeKind::ZStack(super::ZStackNode {
            style: zstack.style,
            passthrough: zstack.passthrough,
            exiting,
        });
        std::mem::take(&mut node.children)
    };

    let plan = stack_reuse_plan(tree, &old_children, &zstack.children);

    // Layers that opted into an automatic exit and are no longer described collapse in place.
    // Every layer gets the same rectangle, so there is no slot to reserve and no sibling to move.
    let retained = exit_retention::plan_positioned_exits(tree, id, &old_children, &plan);

    let slots =
        exit_retention::interleave_exits(&old_children, &plan, zstack.children.len(), &retained);
    let mut new_children = Vec::with_capacity(slots.len());
    for slot in slots {
        match slot {
            exit_retention::ExitSlot::Exiting(child_id) => {
                let child_rect = exit_retention::positioned_exit_rect(tree, child_id);
                exit_retention::adopt_exiting(tree, child_id, id, child_rect, epoch);
                new_children.push(child_id);
            }
            exit_retention::ExitSlot::Live(index) => {
                let child_id = reconcile_element(
                    &mut ReconcileCtx {
                        tree,
                        epoch,
                        focus,
                        overlay_state,
                    },
                    ElementReconcile {
                        reuse: plan[index],
                        parent: Some(id),
                        el: &zstack.children[index],
                        rect,
                    },
                );
                new_children.push(child_id);
            }
        }
    }
    exit_retention::store_retained(tree, id, retained);

    let node = tree.node_mut(id);
    node.children = new_children;

    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::{Element, IntoElement, Key};
    use crate::layout::LayoutEngine;
    use crate::widgets::{Animated, Text, ZStack};

    fn find_by_key(tree: &NodeTree, key: &str) -> Option<NodeId> {
        let key = Key::from(key.to_string());
        tree.iter()
            .find(|node| node.key.as_ref() == Some(&key))
            .map(|node| node.id)
    }

    fn keyed_zstack(order: &[&str]) -> Element {
        order
            .iter()
            .fold(ZStack::new(), |zs, key| {
                zs.child(Text::new((*key).to_string()).key((*key).to_string()))
            })
            .into()
    }

    #[test]
    fn keyed_reorder_preserves_zstack_child_identity() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 4,
        };

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &keyed_zstack(&["a", "b", "c"]),
            bounds,
            None,
        );
        let a_before = find_by_key(&tree, "a").expect("missing key a");
        let b_before = find_by_key(&tree, "b").expect("missing key b");
        let c_before = find_by_key(&tree, "c").expect("missing key c");

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &keyed_zstack(&["c", "a", "b"]),
            bounds,
            None,
        );
        assert_eq!(find_by_key(&tree, "a"), Some(a_before));
        assert_eq!(find_by_key(&tree, "b"), Some(b_before));
        assert_eq!(find_by_key(&tree, "c"), Some(c_before));
    }

    #[test]
    fn keyed_insertion_preserves_existing_zstack_children() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 4,
        };

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &keyed_zstack(&["a", "b", "c"]),
            bounds,
            None,
        );
        let a_before = find_by_key(&tree, "a").expect("missing key a");
        let b_before = find_by_key(&tree, "b").expect("missing key b");
        let c_before = find_by_key(&tree, "c").expect("missing key c");

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &keyed_zstack(&["a", "x", "b", "c"]),
            bounds,
            None,
        );
        assert_eq!(find_by_key(&tree, "a"), Some(a_before));
        assert_eq!(find_by_key(&tree, "b"), Some(b_before));
        assert_eq!(find_by_key(&tree, "c"), Some(c_before));
        assert!(find_by_key(&tree, "x").is_some());
    }

    fn exiting_zstack(order: &[&str]) -> Element {
        order
            .iter()
            .fold(ZStack::new(), |zs, key| {
                zs.child(
                    Animated::new(Text::new((*key).to_string()))
                        .auto_exit(200)
                        .key((*key).to_string()),
                )
            })
            .into()
    }

    #[test]
    fn simultaneous_exits_keep_z_order_while_live_layers_reorder() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 4,
        };

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &exiting_zstack(&["a", "b", "c", "d"]),
            bounds,
            None,
        );
        LayoutEngine::reconcile_with_focus(&mut tree, &exiting_zstack(&["d", "a"]), bounds, None);

        let root = tree.node(tree.root);
        let keys: Vec<&str> = root
            .children
            .iter()
            .map(|id| tree.node(*id).key.as_ref().map_or("", |key| key.as_ref()))
            .collect();
        assert_eq!(keys, ["b", "c", "d", "a"]);
    }
}
