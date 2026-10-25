//! Semantic control pointers, enabled by the application's Interactive policy.
use crate::PointerShape;
use crate::core::node::{NodeId, NodeTree, WidgetNode};

pub(super) fn pointer_at(tree: &NodeTree, hit: NodeId, x: i16, y: i16) -> Option<PointerShape> {
    if tree.scrollbar_target_at(x, y).is_some() {
        return Some(PointerShape::Pointer);
    }
    let mut current = Some(hit);
    while let Some(id) = current {
        let node = tree.node(id);
        if node.kind.is_disabled() {
            return None;
        }
        if let Some(shape) = node.kind.interactive_pointer_shape_at(x, y, node.rect) {
            return Some(shape);
        }
        current = node.parent;
    }
    None
}
