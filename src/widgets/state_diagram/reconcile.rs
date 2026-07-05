use super::{StateDiagram, StateDiagramNode, measure_state_diagram};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};
pub fn reconcile_state_diagram(
    tree: &mut NodeTree,
    id: NodeId,
    diagram: &StateDiagram,
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
            measured: measure_state_diagram(diagram),
        },
        || NodeKind::StateDiagram(Box::new(StateDiagramNode::from(diagram.clone()))),
    )
}
