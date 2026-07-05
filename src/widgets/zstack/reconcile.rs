use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{ElementReconcile, OverlayState, ReconcileCtx, reconcile_element};
use crate::style::Rect;
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
        node.kind = NodeKind::ZStack(super::ZStackNode {
            style: zstack.style,
            passthrough: zstack.passthrough,
        });
        std::mem::take(&mut node.children)
    };

    let plan = stack_reuse_plan(tree, &old_children, &zstack.children);

    let mut new_children = old_children;
    new_children.clear();

    for (child, reuse_id) in zstack.children.iter().zip(plan) {
        let child_id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: reuse_id,
                parent: Some(id),
                el: child,
                rect,
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
    use crate::core::element::{Element, IntoElement, Key};
    use crate::layout::LayoutEngine;
    use crate::widgets::{Text, ZStack};

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
}
