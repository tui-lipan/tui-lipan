use super::{ErDiagram, ErDiagramNode, measure_er_diagram};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};
pub fn reconcile_er_diagram(
    tree: &mut NodeTree,
    id: NodeId,
    diagram: &ErDiagram,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: diagram.width,
            height: diagram.height,
            measured: measure_er_diagram(diagram),
        },
        || NodeKind::ErDiagram(Box::new(ErDiagramNode::from(diagram.clone()))),
    )
}
