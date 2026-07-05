use crate::core::node::{NodeId, NodeKind};
use crate::layout::reconcile::{ElementReconcile, ReconcileCtx, reconcile_element};
use crate::style::Rect;
use crate::widgets::containers::reconcile::stack_reuse_plan;

pub(crate) fn reconcile_flow(
    ctx: &mut ReconcileCtx<'_>,
    id: NodeId,
    flow: &super::Flow,
    rect: Rect,
) -> NodeId {
    let old_children = {
        let node = ctx.tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::Flow(super::FlowNode {
            style: flow.style,
            padding: flow.padding,
            border: flow.border,
            border_style: flow.border_style,
        });
        std::mem::take(&mut node.children)
    };

    let inner = rect.inner(flow.border, flow.padding);

    let plan = stack_reuse_plan(ctx.tree, &old_children, &flow.children);

    let mut child_rects = vec![
        Rect {
            x: inner.x,
            y: inner.y,
            w: 0,
            h: 0,
        };
        flow.children.len()
    ];
    for row in super::pack_rows(flow, rect) {
        for (idx, child_rect) in row.items {
            child_rects[idx] = child_rect;
        }
    }

    let mut new_children = old_children;
    new_children.clear();

    for ((child, reuse_id), child_rect) in flow.children.iter().zip(plan).zip(child_rects) {
        let child_id = reconcile_element(
            ctx,
            ElementReconcile {
                reuse: reuse_id,
                parent: Some(id),
                el: child,
                rect: child_rect,
            },
        );
        new_children.push(child_id);
    }

    let node = ctx.tree.node_mut(id);
    node.children = new_children;

    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::{Element, IntoElement, Key};
    use crate::core::node::{NodeId, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::widgets::{Flow, Text};

    fn find_by_key(tree: &NodeTree, key: &str) -> Option<NodeId> {
        let key = Key::from(key.to_string());
        tree.iter()
            .find(|node| node.key.as_ref() == Some(&key))
            .map(|node| node.id)
    }

    fn keyed_flow(order: &[&str]) -> Element {
        Flow::new()
            .children(
                order
                    .iter()
                    .map(|k| Text::new((*k).to_string()).key((*k).to_string())),
            )
            .into()
    }

    #[test]
    fn keyed_reorder_preserves_flow_child_identity() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };

        LayoutEngine::reconcile_with_focus(&mut tree, &keyed_flow(&["a", "b", "c"]), bounds, None);
        let a_before = find_by_key(&tree, "a").expect("missing key a");
        let b_before = find_by_key(&tree, "b").expect("missing key b");
        let c_before = find_by_key(&tree, "c").expect("missing key c");

        LayoutEngine::reconcile_with_focus(&mut tree, &keyed_flow(&["c", "a", "b"]), bounds, None);
        assert_eq!(find_by_key(&tree, "a"), Some(a_before));
        assert_eq!(find_by_key(&tree, "b"), Some(b_before));
        assert_eq!(find_by_key(&tree, "c"), Some(c_before));
    }

    #[test]
    fn keyed_insertion_preserves_existing_flow_children() {
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };

        LayoutEngine::reconcile_with_focus(&mut tree, &keyed_flow(&["a", "b", "c"]), bounds, None);
        let a_before = find_by_key(&tree, "a").expect("missing key a");
        let b_before = find_by_key(&tree, "b").expect("missing key b");
        let c_before = find_by_key(&tree, "c").expect("missing key c");

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &keyed_flow(&["a", "x", "b", "c"]),
            bounds,
            None,
        );
        assert_eq!(find_by_key(&tree, "a"), Some(a_before));
        assert_eq!(find_by_key(&tree, "b"), Some(b_before));
        assert_eq!(find_by_key(&tree, "c"), Some(c_before));
        assert!(find_by_key(&tree, "x").is_some());
    }
}
