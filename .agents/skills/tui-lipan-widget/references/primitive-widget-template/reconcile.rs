use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::reconcile_simple_leaf;
use crate::style::{LayoutConstraints, Rect};

use super::{#Name#, #Name#Node, measure_#NAME_SNAKE#};

pub fn reconcile_#NAME_SNAKE#(
    tree: &mut NodeTree,
    id: NodeId,
    widget: &#Name#,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        id,
        rect,
        constraints,
        widget.width,
        widget.height,
        measure_#NAME_SNAKE#(widget),
        || NodeKind::#Name#(#Name#Node::from(widget.clone())),
    )
}
